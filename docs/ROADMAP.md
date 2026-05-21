# oxidize-docx — Roadmap

Estado vivo del plan de implementación. La filosofía, arquitectura y módulos del proyecto viven en `CLAUDE.md`; aquí solo el progreso y lo que falta.

**Última revisión:** 2026-05-21
**Rama de trabajo activa:** `feature/phase1-foundation` (contiene Fase 1 y Fase 2; el nombre es legacy y se reorganizará al cerrar Fase 3)

---

## Estado global

| Fase | Estado | Cierre | Tests |
|------|--------|--------|-------|
| 1. Foundation | ✅ Completada | 2026-03-22 | 61 |
| 2. Raw XML Parsing | ✅ Completada | 2026-03-23 | 128 |
| 3. Semantic Resolution | ⏳ Pendiente | — | — |
| 4. RAG Pipeline | ⏳ Pendiente | — | — |
| 5. Extended Features | ⏳ Pendiente | — | — |

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
- [ ] `numbering/resolver.rs` — `NumberingResolver` stateful (`advance(numId, ilvl) -> ListItemInfo`, `reset_deeper_levels`). Debe llamarse en document order.
- [ ] `pipeline/element.rs` — enum `DocxElement` público (Title, Heading, Paragraph, Table, ListItem, Image, Hyperlink, Footnote, Endnote, Comment, Header, Footer, PageBreak, SectionBreak).
- [ ] `pipeline/classifier.rs` — `ClassifierPipeline` single-pass que mantiene heading context y list state. Aplica `StyleResolver` + `NumberingResolver` para producir `DocxElement` desde `RawBody`.
- [ ] `pipeline/table_builder.rs` — Resolución de spans (`vMerge` restart/continue + `gridSpan`), normalización de filas y celdas vacías.
- [ ] `pipeline/list_builder.rs` — Reconstrucción de nesting de listas a partir de `(numId, ilvl)` por párrafo.
- [ ] `DocxDocument::elements()` — API pública que orquesta lo anterior y cachea el resultado en `ParsedPartsCache`.

### Tests requeridos (TDD)

Cada item de tarea entra con su test reproductor antes del código. No se acepta smoke test (validar contenido real, no presencia o tamaño).

- [ ] Style inheritance: paragraph hereda font de `docDefaults` cuando estilo no define.
- [ ] Style inheritance: basedOn chain de 3+ niveles aplica overrides en orden.
- [ ] Style inheritance: ciclo `A→B→A` produce `CircularStyleReference`.
- [ ] Style inheritance: depth > 64 produce `StyleChainTooDeep`.
- [ ] Numbering: 3 párrafos con mismo numId/ilvl=0 → 1, 2, 3.
- [ ] Numbering: subida de ilvl resetea niveles más profundos.
- [ ] Numbering: bullets vs decimal vs lowerRoman emiten `ListType` correcto.
- [ ] Classifier: heading style → `DocxElement::Heading` con `heading_level` correcto.
- [ ] Classifier: párrafo sin estilo → `DocxElement::Paragraph`.
- [ ] Classifier: `parent_heading` se propaga al siguiente bloque.
- [ ] Table builder: `gridSpan=3` colapsa 3 celdas en 1.
- [ ] Table builder: `vMerge=restart` + `vMerge=continue` produce celda lógica vertical.
- [ ] List builder: listas anidadas (`ilvl=0,1,2`) producen jerarquía correcta.
- [ ] `DocxDocument::elements()` sobre fixture real DOCX devuelve elementos esperados (snapshot).

### Riesgos específicos de Fase 3

1. **`vMerge` + `gridSpan` simultáneos**: spec ECMA-376 ambigua, Word produce XML inconsistente entre versiones. Estrategia: cubrir con tests de fixtures reales generados por distintas versiones de Word.
2. **Style chains profundos**: Confluence/Notion exports llegan a 8-10 niveles. Verificar que el límite 64 absorbe sin degradar performance.
3. **LISTNUM / SEQ fields**: requieren evaluación completa de campos, fuera de scope. `display_index: None` documentado y testeado.
4. **`numbering.xml` ausente**: docs que usan solo bullets directos en `pPr/numPr` con `numId=0`. Tratar como párrafo no-lista, no como error.

---

## Fase 4 — RAG Pipeline ⏳

**Entregable:** `DocxDocument::rag_chunks()` y `to_markdown()` operativos.

### Tareas

- [ ] `pipeline/rag.rs` — `RagChunk` con `paragraph_indices` (no `page_numbers`), `element_types`, `heading_context`, `token_estimate`, `is_oversized`.
- [ ] `pipeline/rag.rs` — `DocxRagChunker` híbrido (replicar el de oxidize-pdf, adaptado a `DocxElement`).
- [ ] `pipeline/export.rs` — `MarkdownExporter` (`# Heading`, `- ListItem`, tablas markdown, `[text](url)` para hyperlinks).
- [ ] `pipeline/export.rs` — `PlainTextExporter` (texto plano sin formato).
- [ ] `DocxDocument::rag_chunks()` one-liner.
- [ ] `DocxDocument::rag_chunks_with_profile(ExtractionProfile)` — placeholder; profiles llegan en Fase 5.
- [ ] `DocxDocument::to_markdown()`.
- [ ] `DocxDocument::plain_text()`.

### Tests requeridos (TDD)

- [ ] Chunker: documento con 1 heading + 5 párrafos cortos → 1 chunk con `heading_context` poblado.
- [ ] Chunker: párrafo de 5000 palabras → split en chunks marcados `is_oversized=true` con boundaries en oraciones.
- [ ] Chunker: cambio de heading abre nuevo chunk.
- [ ] Chunker: `paragraph_indices` cubre exactamente los párrafos del chunk (sin gaps).
- [ ] Markdown: heading level 1-6 emite `#` correcto.
- [ ] Markdown: list anidada emite indentación correcta (2 espacios por nivel).
- [ ] Markdown: tabla con header row emite `|---|---|` separador.
- [ ] Markdown: hyperlink emite `[text](url)`.
- [ ] Plain text: ignora formato pero preserva saltos de párrafo.
- [ ] Integration: fixture DOCX real → `rag_chunks()` produce N chunks con texto exacto verificado.

### Riesgos específicos de Fase 4

1. **Token estimation**: `word_count * 1.5` es aproximación. Usuarios de embedding APIs reales pueden necesitar tokenizadores reales (tiktoken). Documentar como aproximación.
2. **Tablas en markdown**: celdas multilinea o con listas dentro no son representables en markdown estándar. Decidir si flatten o emitir HTML.
3. **Hyperlinks en RAG chunks**: ¿incluir URL en el texto del chunk o solo en metadata? Decisión a tomar antes de cerrar Fase 4.

---

## Fase 5 — Extended Features ⏳

**Entregable:** Cobertura de partes OOXML secundarias (footnotes, endnotes, comments, headers/footers, images) y `ExtractionProfile` variants.

### Tareas

- [ ] `word/footnotes_xml.rs` → `FootnoteMap`.
- [ ] `word/endnotes_xml.rs` → `EndnoteMap`.
- [ ] `word/comments_xml.rs` → `CommentMap`.
- [ ] `word/headers_footers.rs` → `Vec<RawParagraph>` por header/footer ref.
- [ ] `word/settings_xml.rs` (si afecta a pipeline; revisar).
- [ ] `images/extractor.rs` — extracción de bytes de `word/media/`.
- [ ] `images/metadata.rs` — `ImageMetadata` (dimensions opcional vía feature `image-metadata`).
- [ ] `DocxDocument::images()` API.
- [ ] `pipeline/profile.rs` — `ExtractionProfile::{Default, Academic, Technical, Minimal}` con variantes de chunking.

### Tests requeridos (TDD)

- [ ] Footnotes: párrafo con `<w:footnoteReference id="1"/>` → `DocxElement::Footnote` con texto correcto.
- [ ] Comments: párrafo con range de comentario → `DocxElement::Comment` con autor y texto.
- [ ] Headers: parsear header section y exponer como `DocxElement::Header`.
- [ ] Images: extraer PNG/JPEG de `word/media/`, validar bytes con magic numbers.
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
