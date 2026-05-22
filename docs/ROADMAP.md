# oxidize-docx — Roadmap

Estado vivo del plan de implementación. La filosofía, arquitectura y módulos del proyecto viven en `CLAUDE.md`; aquí solo el progreso y lo que falta.

**Última revisión:** 2026-05-21
**Rama de trabajo activa:** `feature/phase3-semantic-resolution` (tip mueve con cada cycle TDD de Fase 3)

---

## Estado global

| Fase | Estado | Cierre | Tests |
|------|--------|--------|-------|
| 1. Foundation | ✅ Completada | 2026-03-22 | 61 |
| 2. Raw XML Parsing | ✅ Completada | 2026-03-23 | 128 |
| 3. Semantic Resolution | 🟢 En curso | — | 144 |
| 4. RAG Pipeline | 🟢 Chunker + exporters listos, falta `with_profile` | — | 157 |
| 5. Extended Features | 🟡 Images + footnotes + endnotes listas; comments/headers/profile pendientes | — | 177 |

Criterio transversal de "done" para cualquier fase:
- `cargo check --all-targets` sin warnings.
- `cargo clippy --all-targets -- -D warnings` limpio.
- `cargo fmt --check` limpio.
- `cargo test` verde, sin smoke tests (validar contenido real, no return codes).
- Documentación de la fase actualizada en este archivo.

---

## Fase 1 — Foundation ✅

Cerrada el 2026-03-22 en commit `de18ef5`.

- [x] Workspace `Cargo.toml` con resolver 2 y `workspace.dependencies`.
- [x] `error.rs` con `DocxError` (17 variants) y `Result<T>` alias.
- [x] `zip/` con security checks: bomb detection, entry count, path traversal.
- [x] `ooxml/content_types.rs` y `ooxml/relationships.rs`.
- [x] `xml/reader.rs`, `xml/entity_guard.rs`, `xml/namespace.rs`.
- [x] `document.rs` skeleton con `DocxDocument::open()` validando ZIP + manifests OOXML.
- [x] 61 tests verdes.

---

## Fase 2 — Raw XML Parsing ✅

Cerrada el 2026-03-23 en commit `9f3dd40`.

- [x] `raw/` completo: `RawBody`, `RawParagraph`, `RawRun`, `RawTable`, `RawDrawing`, `RawFieldInst`.
- [x] `styles/table.rs` — `StyleTable`, `StyleEntry`, `StyleType` con lookup por `styleId`.
- [x] `numbering/defs.rs` — `NumberingDefs`, `AbstractNum`, `ConcreteNum` con `resolve(numId, ilvl)`.
- [x] `word/styles_xml.rs` → `StyleTable` (docDefaults, rPr, pPr, basedOn).
- [x] `word/numbering_xml.rs` → `NumberingDefs` (abstractNum, levels, concrete nums).
- [x] `word/document_xml.rs` → `RawBody` (SAX state machine: paragraphs, runs, hyperlinks, tables con gridSpan/vMerge, sectPr).
- [x] `document.rs`: eager XML extraction + lazy `RefCell` caching (`raw_body()`, `style_table()`, `numbering_defs()`).
- [x] `xml/reader.rs::from_bytes_preserve_text()` para preservar whitespace en `<w:t>`.
- [x] Helpers compartidos: `parse_run_properties()`, `parse_paragraph_properties()` (incluye `numPr`).
- [x] 128 tests verdes.

---

## Fase 3 — Semantic Resolution ⏳

Convertir el árbol raw en `Vec<DocxElement>` semánticos. Es la fase que conecta XML crudo con la API pública pensada para humanos y RAG.

**Entregable:** `DocxDocument::elements() -> Result<Vec<DocxElement>>` operativo.

### Tareas

- [ ] `styles/resolver.rs` — `StyleResolver` con cadena de herencia de 4 capas (docDefaults → basedOn chain → list-level → direct). Max depth 64 con detección de ciclos (`CircularStyleReference`).
- [ ] `styles/formatting.rs` — `ResolvedFormatting` con campos finales (bold, italic, font_size en points, heading_level…).
- [x] `numbering/resolver.rs` — `NumberingResolver` stateful (`advance(num_id, ilvl) -> Result<ListItemInfo>`, reset de niveles más profundos inline). Debe llamarse en document order.
- [~] `pipeline/element.rs` — enum `DocxElement` público. Variantes ya añadidas: `Paragraph`, `Heading`, `ListItem`, `Table` + tipos `HeadingContext`, `TableCell`, `TableRow`. Pendientes: `Title`, `Image`, `Hyperlink`, `Footnote`, `Endnote`, `Comment`, `Header`, `Footer`, `PageBreak`, `SectionBreak` (cada una entra con su test, no por adelantado).
- [~] `pipeline/classifier.rs` — `ClassifierPipeline` con `StyleResolver` + `NumberingResolver` + `TableBuilder` integrados; emite `Paragraph`/`Heading`/`ListItem`/`Table` en document order, propaga `current_heading`. Pendiente: gestionar `SectionBreak`, complex fields y drawings.
- [~] `pipeline/table_builder.rs` — `build_table()` resuelve `gridSpan` (→ `col_span`) y `vMerge` Restart/Continue (→ `row_span` colapsado en la celda ancla; las Continue cells no se emiten). Pendiente: cubrir vMerge interrumpido por celda normal, vMerge orphan sin Restart previo (hoy se descarta silenciosamente), y normalización de filas asimétricas para downstream renderers que asuman grid alignment.
- [x] `pipeline/list_builder.rs` — `nest_list_items()` standalone toma `&[DocxElement]` y devuelve `Vec<NestedList>`: agrupa `ListItem` consecutivos en un mismo `NestedList`, los separa cuando aparece un elemento no-lista, y construye el árbol parent/child por `level` con stack de índices (sin alterar la salida plana del classifier — los consumidores que necesiten árbol llaman a la utilidad).
- [~] `DocxDocument::elements()` — API pública ya operativa: orquesta `RawBody` + `StyleTable` + `NumberingDefs` (cacheados vía `RefCell`) y construye un `ClassifierPipeline` transitorio por llamada. Pendiente: cachear `Vec<DocxElement>` para evitar reclasificación en llamadas sucesivas y manejar `RawBodyItem::Table`/`SectionBreak` cuando lleguen `TableBuilder`/`SectionBuilder`.

### Tests requeridos (TDD)

Cada item de tarea entra con su test reproductor antes del código. No se acepta smoke test (validar contenido real, no presencia o tamaño).

- [x] Style inheritance: paragraph hereda font de `docDefaults` cuando estilo no define.
- [x] Style inheritance: basedOn chain de 3+ niveles aplica overrides en orden.
- [x] Style inheritance: ciclo `A→B→A` produce `CircularStyleReference`.
- [x] Style inheritance: depth > 64 produce `StyleChainTooDeep`.
- [x] Numbering: 3 párrafos con mismo numId/ilvl=0 → 1, 2, 3.
- [x] Numbering: subida de ilvl resetea niveles más profundos (mismo `num_id` solo).
- [x] Numbering: bullets vs decimal vs lowerRoman emiten `ListType` correcto (bullet → `display_index=None`).
- [x] Numbering: `level.start > 1` siembra el primer índice (cubre `<w:start w:val="5"/>`).
- [x] Numbering: counters de `num_id` distintos son independientes (reset no contamina otra lista).
- [x] Numbering: `advance` con `num_id` desconocido devuelve `NumberingDefNotFound`.
- [x] Classifier: heading style → `DocxElement::Heading` con `heading_level` correcto.
- [x] Classifier: párrafo sin estilo → `DocxElement::Paragraph` (con `parent_heading: None`).
- [x] Classifier: `parent_heading` se propaga al siguiente bloque (paragraphs, list items via numPr).
- [x] Classifier: `RawBodyItem` con `RawNumPr` produce `DocxElement::ListItem` con `display_index`, `level`, `list_type` resueltos por `NumberingResolver`.
- [x] Classifier: document order preservado en sequencia mixta (Heading → Paragraph → ListItem → Paragraph).
- [x] Table builder: `gridSpan=3` colapsa en una celda con `col_span=3`.
- [x] Table builder: `vMerge=restart` + `vMerge=continue` produce celda ancla con `row_span=2`; la fila de continuación queda con 0 celdas. Combinación `gridSpan=2` + `vMerge` en la misma celda cubierta vía integration test end-to-end.
- [x] List builder: listas anidadas (`ilvl=0,1,0,1,2`) producen árbol con dos roots, cada uno con sus children, y un nieto en la rama derecha. Cubierto además: item único → tree con un nodo sin hijos; dos items mismo nivel → siblings; items separados por Heading → dos `NestedList` distintos (runs cortados por elementos no-list).
- [~] `DocxDocument::elements()` sobre DOCX in-memory cubierto por 4 tests (paragraph mínimo, heading vía styles.xml con `parent_heading` propagado, dos `ListItem` decimales con counter 1→2, tabla con `gridSpan=2` + `vMerge` resueltos). Pendiente: snapshot contra un fixture .docx real con contenido mixto cuando exista.

### Riesgos específicos de Fase 3

1. **`vMerge` + `gridSpan` simultáneos**: spec ECMA-376 ambigua, Word produce XML inconsistente entre versiones. Estrategia: cubrir con tests de fixtures reales generados por distintas versiones de Word.
2. **Style chains profundos**: Confluence/Notion exports llegan a 8-10 niveles. Verificar que el límite 64 absorbe sin degradar performance.
3. **LISTNUM / SEQ fields**: requieren evaluación completa de campos, fuera de scope. `display_index: None` documentado y testeado.
4. **`numbering.xml` ausente**: docs que usan solo bullets directos en `pPr/numPr` con `numId=0`. Tratar como párrafo no-lista, no como error.

---

## Fase 4 — RAG Pipeline ⏳

**Entregable:** `DocxDocument::rag_chunks()` y `to_markdown()` operativos.

### Tareas

- [x] `pipeline/rag.rs::RagChunk` — campos `text`, `paragraph_indices`, `element_types`, `heading_context`, `token_estimate`, `is_oversized`. Cumple la nota de roadmap: usa `paragraph_indices` (no `page_numbers`) porque DOCX no tiene páginas pre-layout.
- [x] `pipeline/rag.rs::DocxRagChunker` — chunker heading-aware (cambio de heading abre chunk nuevo) + size-aware (split de párrafos cuyo token_estimate excede `max_tokens` en sub-chunks con `is_oversized=true`, partiendo en boundaries `.`/`!`/`?`). Estimación `word_count * 1.5` documentada como aproximación. Pendiente: agresividad de chunking inter-elemento (hoy un párrafo que cabe pero deja el chunk levemente sobre el límite no se reasigna).
- [x] `pipeline/export.rs::to_markdown()` — `# Heading` (clamped a 6), paragraphs separados por blank line, list items con indent `2 * level` y marker `N.` para decimal / `-` para todo lo demás, tablas GFM con row 0 como header. Pendiente: emitir `[text](url)` cuando aparezca la variante Hyperlink en `DocxElement`.
- [x] `pipeline/export.rs::to_plain_text()` — bloques separados por blank line, listas tight (single `\n`), celdas de tabla unidas por ` | ` por fila.
- [x] `DocxDocument::rag_chunks()` — one-liner que orquesta `elements()` + `DocxRagChunker::new().chunk()` con defaults (max_tokens=800).
- [ ] `DocxDocument::rag_chunks_with_profile(ExtractionProfile)` — placeholder; profiles llegan en Fase 5.
- [x] `DocxDocument::to_markdown()` — orquesta `elements()` + `to_markdown()`; cubierto por integration test heading + list + paragraph.
- [x] `DocxDocument::plain_text()` — orquesta `elements()` + `to_plain_text()`; cubierto por integration test heading + list + paragraph.

### Tests requeridos (TDD)

- [x] Chunker: heading + paragraph → 1 chunk con `heading_context=[H]` (caso canónico "heading + body").
- [x] Chunker: párrafo cuyo `token_estimate > max_tokens` → split en sub-chunks marcados `is_oversized=true` con boundaries en `.`/`!`/`?`.
- [x] Chunker: cambio de heading mismo nivel abre nuevo chunk (`[H1 A, p, H1 B, p]` → 2 chunks).
- [x] Chunker: `paragraph_indices` contiguos dentro de un chunk y, en la unión de todos los chunks, reproduce `0..elements.len()` sin gaps ni duplicados.
- [x] Markdown: heading level 1-6 emite `#` correcto (loop sobre los 6 niveles en un sólo test).
- [x] Markdown: list anidada emite indentación correcta (2 espacios por nivel, decimal/bullet markers).
- [x] Markdown: tabla con header row emite `|---|---|` separador (GFM).
- [ ] Markdown: hyperlink emite `[text](url)` — bloqueado hasta que `DocxElement::Hyperlink` exista (Fase 5).
- [x] Plain text: ignora formato pero preserva saltos de párrafo y listas tight; tablas en `cells | por | row`.
- [ ] Integration: fixture DOCX real → `rag_chunks()` produce N chunks con texto exacto verificado.

### Riesgos específicos de Fase 4

1. **Token estimation**: `word_count * 1.5` es aproximación. Usuarios de embedding APIs reales pueden necesitar tokenizadores reales (tiktoken). Documentar como aproximación.
2. **Tablas en markdown**: celdas multilinea o con listas dentro no son representables en markdown estándar. Decidir si flatten o emitir HTML.
3. **Hyperlinks en RAG chunks**: ¿incluir URL en el texto del chunk o solo en metadata? Decisión a tomar antes de cerrar Fase 4.

---

## Fase 5 — Extended Features ⏳

**Entregable:** Cobertura de partes OOXML secundarias (footnotes, endnotes, comments, headers/footers, images) y `ExtractionProfile` variants.

### Tareas

- [x] `word/footnotes_xml.rs::parse_footnotes_xml()` → `FootnoteMap` (HashMap<u32, String>). Skip separator/continuationSeparator footnotes; concatena text de `<w:t>` con `preserve_text` para conservar espacios. `RawParagraph` ahora captura `footnote_ref_ids: Vec<u32>` desde `<w:footnoteReference w:id>`. Classifier emite `DocxElement::Footnote { id, text }` inmediatamente después del párrafo que la referencia (via `ClassifierPipeline::with_footnotes`). `DocxDocument::elements()` carga footnotes lazy via `RefCell` y las inyecta al classifier cuando existen.
- [x] `word/endnotes_xml.rs::parse_endnotes_xml()` → `EndnoteMap`. Comparte el parser con footnotes vía `word/notes_common::parse_note_collection(xml, part_name, note_tag)`: el envelope OOXML es estructuralmente idéntico, sólo cambia el nombre del elemento. `RawParagraph` ahora también captura `endnote_ref_ids` desde `<w:endnoteReference>`. Classifier emite `DocxElement::Endnote { id, text }` tras los footnotes del mismo párrafo, vía `with_endnotes()` builder. `DocxDocument::elements()` carga endnotes lazy con su propio `RefCell`.
- [ ] `word/comments_xml.rs` → `CommentMap`.
- [ ] `word/headers_footers.rs` → `Vec<RawParagraph>` por header/footer ref.
- [ ] `word/settings_xml.rs` (si afecta a pipeline; revisar).
- [x] `images/extractor.rs::extract_images()` — enumera entries `word/media/*`, lee bytes vía `SecureZipArchive`, sniffea content_type por magic bytes, ordena por path.
- [x] `images/metadata.rs::ImageMetadata` — `{ path, bytes, content_type }`. `detect_content_type` reconoce PNG, JPEG, GIF87a/89a, BMP, WebP (RIFF/WEBP); fallback `application/octet-stream`. `width`/`height` opcionales via feature `image-metadata` pendientes.
- [x] `DocxDocument::images()` — público, conserva el `SecureZipArchive` en `RefCell` para releer media on-demand sin reabrir el archivo.
- [ ] `pipeline/profile.rs` — `ExtractionProfile::{Default, Academic, Technical, Minimal}` con variantes de chunking.

### Tests requeridos (TDD)

- [x] Footnotes: parser cubre empty / single-user / separator+user (skip) / multi-user / unknown-id (5 unit). Document XML parser captura footnoteReference IDs (1 unit). Classifier emite Footnote tras el párrafo (1 unit). Integration: DOCX completo con `word/footnotes.xml` + paragraph referenciando id=1 → elements() devuelve Paragraph + Footnote.
- [x] Endnotes: parser reutiliza `notes_common` (4 unit tests propios). Document XML captura endnoteReference (1 unit, valida que no contamina footnote_ref_ids). Classifier emite Endnote tras footnotes (1 unit que verifica orden Paragraph→Footnote→Endnote). Integration: DOCX con `word/endnotes.xml` → Paragraph + Endnote end-to-end.
- [ ] Comments: párrafo con range de comentario → `DocxElement::Comment` con autor y texto.
- [ ] Headers: parsear header section y exponer como `DocxElement::Header`.
- [x] Images: detect_content_type cubre PNG, JPEG, GIF87a/89a, BMP, WebP + fallback octet-stream (7 unit tests). Integration: DOCX sin media → empty Vec; DOCX con un PNG → ImageMetadata correcta; DOCX con PNG+JPEG en orden inverso → resultado ordenado por path (3 integration tests).
- [ ] ExtractionProfile::Academic: cita footnotes inline en RAG chunks.
- [ ] ExtractionProfile::Minimal: omite comments y footnotes.

---

## Backlog post-1.0

No comprometidos, fuera de roadmap activo. Se priorizan tras cerrar Fase 5.

- Evaluación completa de complex fields (`LISTNUM`, `SEQ`, `TOC`, `REF`, `PAGEREF`).
- Soporte de `.docm` (macros) — solo lectura del documento, ignorar VBA.
- Roundtrip parcial (read → modify → write) — actualmente fuera de scope.
- Compatibilidad con Open Document Format (`.odt`) vía adapter.
- Detección de idioma por bloque (`xml:lang`).
- Streaming de chunks (`Iterator<Item = RagChunk>`) para documentos muy grandes.

---

## Riesgos conocidos (globales)

Riesgos transversales que cruzan fases. Riesgos específicos de cada fase viven en su sección.

1. **Table cell spans**: `vMerge` + `gridSpan` simultáneos. Spec ambigua, Word inconsistente.
2. **Style chains profundos**: hasta 10 niveles en docs empresariales (Confluence, Notion exports).
3. **LISTNUM/SEQ fields**: requieren evaluación de campos completa, fuera de scope inicial → `display_index: None` documentado.
4. **Charset en text runs**: docs con `<w:fldChar>` y caracteres especiales no Unicode pueden romper parsers ingenuos. `quick-xml` lo cubre, pero verificar con fixtures reales.

---

## Cómo trabajar con este roadmap

1. Al arrancar fase nueva: abrir issue en GitHub con título `Phase N: <Nombre>` enlazando esta sección.
2. Cada tarea con `[ ]` debe llegar con su test antes que su implementación (TDD estricto).
3. Al cerrar tarea, marcar `[x]` aquí en el mismo commit que la implementa.
4. Al cerrar fase, actualizar tabla de "Estado global" con commit hash y test count, y archivar el issue.
