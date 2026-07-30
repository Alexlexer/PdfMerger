# Security Policy

## Supported versions

Security fixes are provided for the latest release and the current `main` branch.
Older releases may require upgrading before a fix is available.

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Use
[GitHub private vulnerability reporting](https://github.com/Alexlexer/PdfMerger/security/advisories/new)
to share the affected version, reproduction steps, impact, and any suggested
mitigation.

Security-sensitive examples include code execution, unintended local file
access, password disclosure, unsafe handling of crafted PDF or image files, and
project files escaping their expected directories.

The maintainers aim to acknowledge a report within seven days. Confirmed issues
will be coordinated privately until a fix and disclosure plan are ready.

## Scope

PdfMerger processes untrusted document data locally. Reports about malformed or
hostile PDFs, images, or project files are welcome. Findings that require a
modified build, an already-compromised operating system, or unsupported
platforms may be treated as out of scope.
