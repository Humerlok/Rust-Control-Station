/**
 * Browser-only stand-in for the native "Save as" dialog: the mock backend (`?mock=1`) has no
 * file system, so saves become a regular browser download and resolve to the file name.
 */
export function browserDownload(filename: string, data: BlobPart, type = "application/octet-stream"): string {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  return filename;
}
