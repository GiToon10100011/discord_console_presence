export const PLATFORMS = [
  "Nintendo Switch",
  "Nintendo Switch 2",
  "PlayStation 5",
  "PlayStation 4",
  "Xbox Series X|S",
  "Xbox One",
  "PC",
  "기타",
] as const;

/** Brand-ish accent per console family, used for the platform badges. */
export function platformColor(platform: string): string {
  const p = platform.toLowerCase();
  if (p.includes("switch") || p.includes("nintendo")) return "#e60012";
  if (p.includes("playstation") || p.includes("ps")) return "#0070d1";
  if (p.includes("xbox")) return "#107c10";
  if (p.includes("pc") || p.includes("steam")) return "#6b7280";
  return "#7c3aed";
}
