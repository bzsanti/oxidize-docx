# Design — Alinear el chunking RAG de oxidize-docx con oxidize-pdf

**Fecha:** 2026-07-02
**Rama:** `feature/rag-align-oxidize-pdf`
**Estado:** aprobado (diseño + 3 decisiones embebidas)

## Contexto y motivación

`CLAUDE.md` afirma que el chunker de docx es "hybrid chunker, misma implementación
que oxidize-pdf". Es falso: divergen en `max_tokens` (800 vs 512), en el token math
(×1.5 vs word count crudo) y — el gap real — docx **no aplica cap de tamaño
inter-elemento**. El cap solo dispara cuando un elemento *individual* excede el
budget (`rag.rs:109`); una sección larga sin headings produce un chunk
arbitrariamente grande. oxidize-pdf es el punto de referencia del ecosistema; este
trabajo alinea el algoritmo y la metadata de chunking con `oxidize-pdf-core`.

Referencia pdf: `oxidize-pdf-core/src/pipeline/hybrid_chunking.rs`,
`pipeline/chunk_metadata.rs`, `pipeline/profile.rs`.

## Alcance

Paridad total del **subconjunto aplicable a docx**. Se porta lo format-agnostic;
se excluye lo que es puro dominio-PDF y no tiene fuente de datos en docx.

| Se porta | Se excluye (sin equivalente docx) |
|---|---|
| HybridChunker: cap inter-elemento, merge policy, element-type awareness, section-grouping | `dominant_font`, bold/italic % (docx no expone run formatting en `DocxElement`) |
| `chunk_id` (hash), `prev/next_chunk_id`, `heading_path`, content-type flags, `full_text` | `bounding_boxes`, `page_span`, `page_regions` (docx no tiene coordenadas ni páginas) |
| `chunk_with_graph` (agrupar sección bajo heading) | `overlap_tokens` (muerto en pdf; no se replica — chunks disjuntos) |

**Token model:** se mantiene ×1.5 (mejor proxy de BPE que el word count crudo de
pdf; ambas libs comparten el TODO de tokenizer real). `max_tokens` default sigue
**800** (≈533 words, mismo orden que el budget 512-words de pdf); se documenta la
equivalencia. No se persigue paridad numérica exacta — es una decisión deliberada.

### Decisiones embebidas aprobadas

1. Nueva dependencia `sha2` (pure Rust, respeta "sin C/FFI") para `chunk_id`.
2. Bump a **v0.2.0**: el rename del enum de profiles rompe la API pública
   (pre-1.0, un minor admite breaking).
3. Set de profiles reducido a 5 (sin `Form`/`Government`/`Presentation`, que en
   pdf son puro partitioning espacial → serían no-ops en docx).

## Componentes (unidades)

`pipeline/rag.rs` (~290 LOC) crece; se separa en unidades de propósito único:

- **`pipeline/rag.rs`** — `RagChunk` (envelope de salida), `DocxRagChunker`
  (config + API pública), orquestación `chunk()` / `chunk_with_graph()`.
- **`pipeline/hybrid.rs`** (nuevo) — el algoritmo de packing: clasificación
  structural/inline, greedy con cap, merge policy, split de oversized,
  section-grouping. Sin estado público; consumido por `DocxRagChunker`.
- **`pipeline/chunk_metadata.rs`** (nuevo) — `ChunkMetadata`, `ContentTypeFlags`,
  cálculo de `chunk_id` (SHA-256 de `full_text`), `heading_path`, y `link_chunks()`
  (linkeo post-hoc de prev/next).

Cada unidad: qué hace, cómo se usa, de qué depende — testeable en aislamiento.

## Algoritmo (mirror de `hybrid_chunking.rs`)

Greedy packing sobre `&[DocxElement]` en document order. Clasificación por elemento:

- **Structural (siempre cortan buffer, emiten chunk propio):** `Heading`, `Table`.
  `Header`/`Footer` se descartan (ruido repetido page-level, comportamiento actual).
- **Inline (mergeables):** `Paragraph`, `ListItem`, `Footnote`, `Endnote`, `Comment`.

Reglas:
- Un elemento inline se añade al buffer solo si
  `buffer_tokens + elem_tokens <= max_tokens` **y** la `MergePolicy` lo permite.
- `MergePolicy::AnyInlineContent` (default): une cualquier par de inlines.
  `MergePolicy::SameTypeOnly`: solo une elementos del mismo `element_type`.
- Al desbordar, o ante un elemento structural, o merge deshabilitado → flush.
- **Oversized** (elemento solo con `estimate_tokens > max_tokens`): flush del buffer;
  si es splittable (`Paragraph`/`ListItem`) → `split_sentences` + `pack_sentences`
  (ya existen en rag.rs), cada fragmento como chunk `is_oversized=true`; si no
  (`Table`) → chunk atómico `is_oversized=true`.
- **`chunk_with_graph`:** agrupa todos los elementos bajo un `Heading` (hasta el
  siguiente heading de nivel <=) en una sección. Si `section_tokens <= max_tokens`
  → 1 chunk. Si no → delega en el greedy y re-estampa `heading_path` en cada
  sub-chunk. El preámbulo antes del primer heading se chunkea normal.

Config nueva en `DocxRagChunker` (defaults entre paréntesis):
`merge_adjacent: bool` (true), `propagate_headings: bool` (true),
`merge_policy: MergePolicy` (`AnyInlineContent`).

## Metadata

`RagChunk` gana campos (construidos por el chunker, no por literales — los tests
existentes que asertan campos concretos siguen válidos):

- `chunk_index: usize`
- `full_text: String` — `heading_path.join(" > ") + "\n\n" + text` para embedding.
- `chunk_id: String` — determinista `{doc_id}:{chunk_index}`, `doc_id` = primeros
  8 bytes de `SHA-256(full_text)` en 16 hex chars.
- `prev_chunk_id: Option<String>`, `next_chunk_id: Option<String>` — linkeados
  post-hoc por `link_chunks()` tras generar todos los chunks.
- `heading_path: Vec<String>` — breadcrumb root→leaf (los `.text` del
  `heading_context` ya existente; se deriva, no se recomputa la pila).
- `content_types: ContentTypeFlags { has_table, has_list, heading_only }` — sin
  `has_code` (docx no tiene CodeBlock).

Se mantienen los campos actuales: `text`, `paragraph_indices`, `element_types`,
`heading_context`, `token_estimate`, `is_oversized`.

## Profiles (espejo de nombres pdf, comportamiento docx-real)

Enum público `ExtractionProfile` pasa a 5 variantes. Cada una con comportamiento
documentado y real — cero knobs muertos.

| Variante | = pdf? | Comportamiento docx | Migración |
|---|---|---|---|
| `Standard` (default) | ✅ | passthrough; notas como bloques propios | era `Default` |
| `Rag` | ✅ | default recomendado RAG: section-grouping on, drop headers/footers | nueva |
| `Academic` | ✅ | inline footnotes/endnotes en la prosa del host | sin cambio |
| `Dense` | ✅ | drop marginalia: footnotes/endnotes/comments | era `Minimal` |
| `Technical` | ✗ docx-only | tablas intactas sin importar budget (no las parte) | sin cambio |

`Form`/`Government`/`Presentation` de pdf se omiten: son puro partitioning espacial,
inaplicable en docx (la estructura viene explícita del XML). Incluirlos sería
no-op — violación de la regla "cero knobs muertos".

## Manejo de errores

Sin nuevos modos de error. El hashing SHA-256 es infalible sobre bytes en memoria.
`link_chunks()` opera sobre un `Vec` ya materializado. La clasificación
structural/inline es total sobre el enum `DocxElement` (match exhaustivo; `Hyperlink`
—deprecated, el classifier ya no lo emite— y `Header`/`Footer` tienen rama explícita).

## Testing (TDD estricto)

Cada capa entra con test rojo antes del código. Sin smoke tests — validar contenido
real de chunks.

1. **Cap inter-elemento:** N paragraphs cuya suma excede `max_tokens` con budget
   pequeño → parten en >1 chunk, cada uno `<= max_tokens`; unión de
   `paragraph_indices` reproduce `0..len` sin gaps/dups (invariante ya testeada).
2. **Merge policy:** `SameTypeOnly` no une `Paragraph`+`ListItem` adyacentes;
   `AnyInlineContent` sí.
3. **Structural break:** un `Table` entre dos paragraphs corta el buffer (Table en
   su chunk).
4. **Section-grouping:** heading + hijos que caben → 1 chunk; que no caben →
   sub-chunks con el mismo `heading_path`.
5. **Metadata:** `chunk_id` determinista y estable ante misma entrada; `prev/next`
   forman cadena doblemente enlazada correcta; `content_types` refleja presencia
   real de table/list; `heading_path` = breadcrumb esperado.
6. **Profiles:** `Standard` idéntico a no-profile; `Dense` colapsa
   Paragraph+Footnote+Endnote+Comment a 1 chunk "paragraph"; `Academic` inlina
   `(Note N: ...)`; `Technical` no parte una tabla oversized; `Rag` dropea
   headers/footers y activa section-grouping.
7. **Integration:** fixtures reales de `.private/fixtures/` (32 docs Word) →
   `rag_chunks()` respeta el cap y produce chunk_ids únicos. Investigar de paso el
   pendiente `empty_chunks` (Plan Hipatia/SFResume/UAT) que puede estar ligado a la
   ausencia de cap.

## Versionado y release

- Bump `0.1.0` → `0.2.0` en el workspace `Cargo.toml` como primer commit del ciclo
  (regla: bump antes del primer PR).
- Gitflow: `feature/rag-align-oxidize-pdf` → `develop` → `main` + tag `v0.2.0`.
- Actualizar `CLAUDE.md`: corregir la afirmación "misma implementación que
  oxidize-pdf" para reflejar el token model divergente (×1.5) y la paridad de
  algoritmo/metadata.
- Actualizar `docs/ROADMAP.md`: cerrar el pendiente de "agresividad de chunking
  inter-elemento" (Fase 4) y registrar la alineación.

## Fuera de scope

- Tokenizer real (tiktoken/BPE) — TODO compartido con pdf, backlog.
- Metadata de fonts/coordenadas/páginas — sin fuente en docx.
- `overlap_tokens` — muerto en pdf; chunks disjuntos por diseño.
- Language detection (`whatlang`) — feature-gated en pdf; no se porta ahora.
- Profiles `Form`/`Government`/`Presentation` — inaplicables en docx.
