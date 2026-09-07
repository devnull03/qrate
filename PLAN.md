# IIIF Presentation API 3

## Purpose

Export IIIF Presentation API 3 manifests from a qrate project. qrate describes and links to
collection resources; it does not operate an IIIF Image API service and must not claim Image API
compliance.

## Scope

1. Define the supported column-to-manifest mapping and representative fixture projects.
2. Export deterministic Presentation 3 manifests, canvases, annotations, metadata, rights, and
   linked media from configured qrate columns.
3. Validate the supported profile before export and report missing or incompatible mappings through
   qrate diagnostics.
4. Document how an existing IIIF Image API service URL may be referenced when the project has one.

## Non-goals

- Hosting image tiles, media, or an IIIF Image API service.
- Importing arbitrary external manifests. Import is a separately scoped future task.
- A new broad project-profile format; project column configuration remains the mapping source of
  truth.

## Dependencies and merge notes

The export and validation core can proceed independently of `gpui-kit-migration`. Any new mapping
editor or Problems-panel interaction must be rebased on that branch before merge.

## Definition of done

- Fixture projects produce valid, stable Presentation 3 JSON.
- Invalid mappings produce actionable diagnostics instead of malformed output.
- Automated tests cover the supported mapping and documented omissions.
