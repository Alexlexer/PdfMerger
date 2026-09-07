# Local PDF summarization

Choose **Selected pages**, **Document group**, or **Original PDF** explicitly in
the Local AI dialog. Selected pages is the default; an empty selection is an error.
Groups can contain pages from several PDFs. Original PDF includes pages removed
from the workspace. Standalone image items are not yet a supported summary scope.

LM Studio discovery uses loopback only. Changing the port invalidates the old
server and model selection; use Refresh to reconnect. The server keeps ownership
of its models. Proxies and redirects remain disabled. API authentication is not
supported by this integration.

## Context and synthesis

Set the context budget at or below the **loaded** model's context size. The setting
is an input budget, not a request to resize LM Studio's model context. The LM Studio
HTTP integration has no verified tokenization endpoint, so sizing conservatively
reserves one token per UTF-8 byte plus role/template overhead, instructions, page
markers, and output tokens. Context-overflow responses trigger smaller requests.

Adjacent pages are packed together; oversized text splits at paragraph, sentence,
or word boundaries, with a UTF-8 boundary fallback. No source fragment is dropped
to fit. Section notes undergo a final synthesis, with recursive reduction when
needed. Short, Standard, and Detailed control the final synthesis. Non-converging
reduction or an answer ending at the token limit fails explicitly rather than
showing a complete summary. This can require disabling thinking in LM Studio.

The built-in GGUF backend remains local and tokenizes its prompts. A section that
cannot fit now fails visibly instead of truncating it silently. Built-in output
that exhausts its token allowance also fails explicitly.

## References and coverage

A job assigns stable reference IDs to `(source PDF path, original page number)`.
Source identity and original numbers are included with extracted text; the original
numbers never depend on workspace ordering. Clicking a citation selects the page
in the workspace, when present, and opens a native view of the original source
page. Copy Summary includes the source mapping and coverage warnings.

Citation validation checks that references actually appeared in the output and
belong to supplied pages. It does **not** verify whether a claim is factually
supported by the cited text. Invalid references and missing references are warned
about. Source files must remain available and unchanged to review their pages.

Extraction coverage lists processed, skipped, failed, truncated, and OCR pages
separately. “Processed” means usable text was extracted, not that every fact was
retained or verified. Partial summaries are labelled. Extraction is capped at
512 MiB per PDF, 16 MiB per page content extraction, 100,000 characters per page,
and 2,000,000 characters across the job. Unread pages after the character limit are
listed as skipped/truncated. If the limit prevents reading another source file,
the job fails and requests a smaller scope. Cancellation does not publish a result.

## AI reading of scanned pages

**LM Studio AI vision** is the default scan reader. Select a model marked **vision**
(the app reads this capability from LM Studio). When normal PDF text extraction
fails, a bounded renderer turns the page into a JPEG and sends it to that local
model. The model transcribes the original language; the ordinary summarization
pipeline then synthesizes the recovered text with page references. No Tesseract or
language packs are required for this mode. Language/handwriting accuracy depends
on the selected model and image quality; it is not guaranteed for every language.
The built-in GGUF integration is text-only and cannot use this image pathway.

AI-read pages are listed separately from Tesseract pages. The same 50-page job cap
and cancellable renderer apply. Image inference uses the configured loopback
server, never a cloud OCR endpoint. Output ending at a token limit is retried up to
three times with increased allowance when the server reports actual prompt usage;
it is never accepted as a complete transcription if still cut off. The same
bounded retry now applies to summarization, including reasoning models.

Initial discovery checks port 1234 and then 1235. Once a server is found or a port
is explicitly configured, Refresh checks that port only.

## Optional Tesseract OCR

Install [Tesseract](https://tesseract-ocr.github.io/tessdoc/Installation.html) and
appropriate language data yourself. PdfMerger does not download or bundle them.
The executable is discovered on PATH (also the standard Program Files location on
Windows); use Check Tesseract after installation. Language codes include `eng`,
`fra`, or `eng+fra`. Availability of the executable does not guarantee that the
chosen language data is installed.

Tesseract is opt-in and only attempts pages without usable extracted text. It
uses the existing Hayro renderer in a private child process, followed by local
Tesseract. Each stage has a 45-second timeout and is killed on cancellation. A job
attempts at most 50 OCR pages, one at a time. Rendering is capped at a 2400-pixel
long edge; OCR uses one OpenMP thread and caps text output at 1 MiB per page.
Temporary files are removed on success, failure, and cancellation. Unix temporary
directories are private to the user. Passwords use stdin, never command arguments
or logs. These are workload limits, not an OS-level memory sandbox; complex PDF
resources can still require significant memory in the renderer child.

Missing OCR, missing language data, OCR failure, or the OCR page cap produce
coverage warnings while ordinary PDF editing and text-based summarization remain
available. OCR text may contain recognition errors: verify exact dates and amounts.

## Manual verification

1. Start LM Studio with a small local language model already loaded; use synthetic
   PDFs for initial checks. No model download or real-document inference is needed
   for the automated suite.
2. Select pages from two PDFs with overlapping original page numbers. Confirm the
   scope and click their citations; each must open the correct source page.
3. Try a long synthetic PDF at a 4096-token budget, then compare Short and Detailed.
   Check section/synthesis progress, one final summary, and retained dates/amounts.
4. Deselect every page: Summarize must remain disabled. Try Group and Original
   explicitly and check their different scope descriptions.
5. Change the LM Studio port; the old server/model must become unusable until
   Refresh succeeds. Cancel during generation and confirm no result is published.
6. With OCR disabled/missing, summarize a mixed text/scanned PDF and check partial
   coverage. Install Tesseract/language data, enable OCR, and verify the OCR page list.
   Cancel during OCR and verify the child process terminates.
7. Repeat a small text-only case using the built-in GGUF backend. Check citations
   and failure handling with a deliberately small output/context allowance.

Automated tests use synthetic PDF fixtures, a deterministic completion client,
mock HTTP servers, and the actual renderer subprocess. They do not establish live
model summary quality, OCR recognition accuracy, or compatibility with untested
server APIs such as PAIR.
