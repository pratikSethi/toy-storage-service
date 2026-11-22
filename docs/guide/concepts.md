# Concepts

## Bucket

A bucket is a named container for objects. Names are 3 to 63 characters: lowercase letters, digits and hyphens. A bucket must be empty before you can delete it.

**Location.** Every bucket lives in a single location today. Choosing a region (and later an availability zone) per bucket is planned, see the [backlog](../backlog.md). It is not part of the API yet.

## Object

An object is one file plus a little information about it: size, content type, ETag, creation time. Objects are stored whole. To change one, upload it again.

## Key

The key is the object's identifier inside its bucket. It looks like a path, for example `reports/2026/q3.csv`, but there are no folders. The slash is an ordinary character.

Listing "everything under `reports/2026/`" is a **prefix listing**: you ask for keys that start with that text, in alphabetical (bytewise) order. Listings come in pages of up to 1000 keys.

Keys cannot be empty or contain `..` segments.

## ETag

The ETag identifies the exact bytes of an object. It is the SHA-256 of the content, in hex. Two uploads of the same bytes have the same ETag, so you can compare it to check that a download matches what you uploaded.

## Versions on overwrite

Uploading to a key that already exists replaces the object. Each overwrite increases the object's `version` by one. Only the latest version is readable today. Keeping and reading old versions is on the [backlog](../backlog.md).

After an upload returns `201`, a read of that key returns the new bytes.

## Errors

Errors are JSON in the `application/problem+json` format ([RFC 9457](https://www.rfc-editor.org/rfc/rfc9457)). Each has a `type`, a `title`, an HTTP `status` and a `detail`, and every response carries a request id you can quote when you ask us about it.

| Status | Meaning |
|---|---|
| 400 | Bad bucket name or key |
| 404 | Bucket or object does not exist |
| 409 | Bucket is not empty, so it cannot be deleted |
| 413 | Object is bigger than the [size limit](limits.md) |
| 416 | The byte range you asked for cannot be served |
| 5xx | Our fault. These count against our [availability target](service-levels.md) |
