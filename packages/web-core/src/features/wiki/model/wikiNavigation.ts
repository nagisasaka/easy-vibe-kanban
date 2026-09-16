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

/** Cancel the renderer's external-link behavior before selecting a Wiki page. */
export function activateWikiLink(
  destination: string | null,
  event: { preventDefault(): void; stopPropagation(): void },
  selectPage: (path: string) => void
): void {
  if (!destination) return;
  event.preventDefault();
  event.stopPropagation();
  selectPage(destination);
}

export type WikiTreeNode =
  | { kind: 'directory'; path: string; name: string; children: WikiTreeNode[] }
  | { kind: 'page'; path: string; page: WikiPage };

/** Build navigation only from the public Wiki snapshot, never a broader file API. */
export function buildWikiTree(pages: readonly WikiPage[]): WikiTreeNode[] {
  const root: WikiTreeNode[] = [];
  for (const page of [...pages].sort((a, b) => a.path.localeCompare(b.path))) {
    const segments = page.path.split('/');
    let siblings = root;
    let path = '';
    for (const name of segments.slice(0, -1)) {
      path = path ? `${path}/${name}` : name;
      let directory = siblings.find(
        (node) => node.kind === 'directory' && node.path === path
      );
      if (!directory) {
        directory = { kind: 'directory', path, name, children: [] };
        siblings.push(directory);
      }
      if (directory.kind === 'directory') siblings = directory.children;
    }
    siblings.push({ kind: 'page', path: page.path, page });
  }
  const sort = (nodes: WikiTreeNode[]): WikiTreeNode[] =>
    nodes
      .sort((a, b) => {
        const rank = (node: WikiTreeNode) =>
          node.kind === 'page' && node.path.split('/').at(-1) === 'index.md'
            ? 0
            : node.kind === 'directory'
              ? 1
              : 2;
        return rank(a) - rank(b) || a.path.localeCompare(b.path);
      })
      .map((node) =>
        node.kind === 'directory'
          ? { ...node, children: sort(node.children) }
          : node
      );
  return sort(root);
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

function normaliseSegments(value: string, openwiki = false): string | null {
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
  return openwiki || result === 'index.md' || result.startsWith('pages/')
    ? result
    : null;
}

export function resolveWikiHref(
  currentPath: string,
  href: string,
  availablePaths: ReadonlySet<string>,
  openwiki = false
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
  const wikiAbsolute = openwiki && decoded.startsWith('/openwiki/');
  if (wikiAbsolute) decoded = decoded.slice('/openwiki/'.length);
  if (decoded.startsWith('/') || /^[a-z]:/i.test(decoded)) return null;
  const base = currentPath.includes('/')
    ? currentPath.slice(0, currentPath.lastIndexOf('/') + 1)
    : '';
  const candidates = wikiAbsolute
    ? [decoded]
    : openwiki
      ? [`${base}${decoded}`, decoded]
      : [decoded, `${base}${decoded}`];
  for (const candidate of candidates) {
    const withExtension =
      openwiki && (candidate === '' || candidate.endsWith('/'))
        ? `${candidate}index.md`
        : candidate.endsWith('.md')
          ? candidate
          : `${candidate}.md`;
    const normalised = normaliseSegments(withExtension, openwiki);
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
