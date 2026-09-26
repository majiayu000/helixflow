# Media fan-in (Tapnow-aligned)

Content nodes are single values. Lists are multiple incoming edges on a `many` port.

## Ports

- `PortDefinition.cardinality`: `one` (default) or `many`.
- `one`: still reject `DuplicateInputConnection`.
- `many`: multiple same-type edges allowed; one edge is a list of length 1.

## Nodes

- Keep `input.text`, `input.image`.
- Add `input.video` and `input.audio` (`storage_uri`, `upload://`).
- `video.image_to_video` inputs:
  - `image` IMAGE many required (1+ stills / r2v)
  - `video` VIDEO many optional
  - `audio` AUDIO many optional
  - `prompt` TEXT one optional

## Ingest

Upload `file` accepts png/jpeg/webp, mp4/webm, wav/mp3. Same `upload://` contract.

## UI

`many` ports stay connectable when occupied (no replace confirm). Upload picker accepts image/video/audio.
