/**
 * Bytes as a reader would say them.
 *
 * Base-1024 with base-10 unit names, which is what every OS file manager on
 * the platforms this ships to shows. Whole numbers from 10 up, one decimal
 * below that: "383 MB", "1.4 GB", "512 B".
 */
export function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value >= 10 || unitIndex === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unitIndex]}`;
}
