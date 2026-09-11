import type {
  RepoWithTargetBranch,
  WikiPage,
  WikiSnapshot,
  Workspace,
} from 'shared/types';

const EXTERNAL_SCHEME_RE = /^[a-z][a-z0-9+.-]*:/i;

export interface WikiRepositoryOption {
  id: string;
  name: string;
}

export function wikiRepositoryOptions(
  workspace: Workspace,
  repos: readonly RepoWithTargetBranch[]
): WikiRepositoryOption[] {
  if (repos.length > 0) {
    return repos.map((repo) => ({ id: repo.id, name: repo.display_name }));
  }
  if (workspace.workspace_kind === 'direct_folder') {
    return [
      {
        id: workspace.id,
        name: workspace.name ?? 'Direct folder',
      },
    ];
  }
  return [];
}

export function pageTitle(page: WikiPage): string {
  return (
    page.metadata?.title ??
    page.path.split('/').at(-1)?.replace(/\.md$/i, '') ??
    page.path
  );
}

export function allWikiPages(snapshot: WikiSnapshot): WikiPage[] {
  return [...(snapshot.index ? [snapshot.index] : []), ...snapshot.pages];
}

export function searchWikiPages(
  snapshot: WikiSnapshot,
  query: string
): WikiPage[] {
  const terms = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return allWikiPages(snapshot).filter((page) => {
    if (terms.length === 0) return true;
    const metadata = page.metadata;
    const haystack = [
      page.path,
      page.content,
      metadata?.title,
      metadata?.summary,
      ...(metadata?.tags ?? []),
      ...(metadata?.sources ?? []),
      ...(metadata?.repos ?? []),
    ]
      .filter(Boolean)
      .join(' ')
      .toLocaleLowerCase();
    return terms.every((term) => haystack.includes(term));
  });
}

function normaliseSegments(value: string): string | null {
  const segments: string[] = [];
  for (const segment of value.replace(/\\/g, '/').split('/')) {
    if (!segment || segment === '.') continue;
    if (segment === '..') {
      if (segments.length === 0) return null;
      segments.pop();
      continue;
    }
    segments.push(segment);
  }
  const result = segments.join('/');
  return result === 'index.md' || result.startsWith('pages/') ? result : null;
}

export function resolveWikiHref(
  currentPath: string,
  href: string,
  availablePaths: ReadonlySet<string>
): string | null {
  if (!href || href.startsWith('#') || EXTERNAL_SCHEME_RE.test(href)) {
    return null;
  }
  let decoded: string;
  try {
    decoded = decodeURIComponent(href.split(/[?#]/, 1)[0]);
  } catch {
    return null;
  }
  if (decoded.startsWith('/') || /^[a-z]:/i.test(decoded)) return null;
  const base = currentPath.includes('/')
    ? currentPath.slice(0, currentPath.lastIndexOf('/') + 1)
    : '';
  const candidates = [decoded, `${base}${decoded}`];
  for (const candidate of candidates) {
    const withExtension = candidate.endsWith('.md')
      ? candidate
      : `${candidate}.md`;
    const normalised = normaliseSegments(withExtension);
    if (normalised && availablePaths.has(normalised)) return normalised;
    const underPages = normaliseSegments(`pages/${withExtension}`);
    if (underPages && availablePaths.has(underPages)) return underPages;
  }
  return null;
}

export function expandWikiLinks(markdown: string): string {
  return markdown.replace(
    /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g,
    (_match, rawTarget: string, rawLabel?: string) => {
      const target = rawTarget.trim();
      const label = rawLabel?.trim() || target;
      const href = target.endsWith('.md') ? target : `${target}.md`;
      return `[${label}](${href})`;
    }
  );
}
