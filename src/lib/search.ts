/** Split the search box into include text and inline `-exclude` tokens. */
export function parseSearchQuery(raw: string): {
  include: string;
  excludeTerms: string[];
} {
  const excludeTerms: string[] = [];
  const includeParts: string[] = [];
  for (const token of raw.trim().split(/\s+/)) {
    if (!token) continue;
    if (token.startsWith("-") && token.length > 1) {
      excludeTerms.push(token.slice(1));
    } else {
      includeParts.push(token);
    }
  }
  return { include: includeParts.join(" "), excludeTerms };
}
