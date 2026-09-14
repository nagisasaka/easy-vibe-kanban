import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
} from 'react';
import type {
  RepoWithTargetBranch,
  WikiPage,
  Workspace,
  WorkspaceWikiSnapshot,
} from 'shared/types';
import { ArrowClockwiseIcon, BookOpenIcon } from '@phosphor-icons/react';
import { workspacesApi } from '@/shared/lib/api';
import { WikiBootstrapDialog } from './WikiBootstrapDialog';
import { MarkdownPreview } from '@/shared/components/MarkdownPreview';
import { getResolvedTheme, useTheme } from '@/shared/hooks/useTheme';
import {
  allWikiPages,
  expandWikiLinks,
  pageTitle,
  resolveWikiHref,
  searchWikiPages,
  wikiRepositoryOptions,
} from '../model/wikiNavigation';

interface WorkspaceWikiPanelProps {
  workspace: Workspace;
  repos: RepoWithTargetBranch[];
}

const LANGUAGE_OPTIONS = [
  ['en', 'English'],
  ['ja', '日本語'],
  ['de', 'Deutsch'],
  ['es', 'Español'],
  ['fr', 'Français'],
  ['ko', '한국어'],
  ['pt-BR', 'Português (Brasil)'],
  ['zh-Hans', '简体中文'],
  ['zh-Hant', '繁體中文'],
] as const;

export function WorkspaceWikiPanel({
  workspace,
  repos,
}: WorkspaceWikiPanelProps) {
  const { theme } = useTheme();
  const resolvedTheme = getResolvedTheme(theme);
  const repositoryOptions = useMemo(
    () => wikiRepositoryOptions(workspace, repos),
    [repos, workspace]
  );
  const [repoId, setRepoId] = useState(repositoryOptions[0]?.id ?? '');
  const formatKey = `evk-wiki-viewer-format:${workspace.id}`;
  const [openwiki, setOpenwiki] = useState(() => {
    try {
      return sessionStorage.getItem(formatKey) === 'openwiki';
    } catch {
      return false;
    }
  });
  const [snapshot, setSnapshot] = useState<WorkspaceWikiSnapshot | null>(null);
  const [selectedPath, setSelectedPath] = useState('index.md');
  const [query, setQuery] = useState('');
  const [language, setLanguage] = useState('en');
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [bootstrapOpen, setBootstrapOpen] = useState(false);
  const loadSequence = useRef(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!repositoryOptions.some((repo) => repo.id === repoId)) {
      setRepoId(repositoryOptions[0]?.id ?? '');
    }
  }, [repoId, repositoryOptions]);

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    if (!repoId) {
      setSnapshot(null);
      return;
    }
    setLoading(true);
    setError(null);
    setSnapshot(null);
    try {
      const next = await workspacesApi.wiki.snapshot(
        workspace.id,
        repoId,
        openwiki
      );
      if (sequence !== loadSequence.current) return;
      setSnapshot(next);
      setLanguage(next.wiki.config?.output_language ?? 'en');
      const paths = new Set(allWikiPages(next.wiki).map((page) => page.path));
      setSelectedPath((current) =>
        paths.has(current)
          ? current
          : (next.wiki.index?.path ?? next.wiki.pages[0]?.path ?? 'index.md')
      );
    } catch (reason) {
      if (sequence !== loadSequence.current) return;
      setError(
        reason instanceof Error ? reason.message : 'Unable to load Wiki'
      );
    } finally {
      if (sequence === loadSequence.current) setLoading(false);
    }
  }, [repoId, workspace.id, openwiki]);

  useEffect(() => {
    void load();
    return () => {
      loadSequence.current += 1;
    };
  }, [load]);

  const pages = useMemo(
    () => (snapshot ? searchWikiPages(snapshot.wiki, query) : []),
    [query, snapshot]
  );
  const allPages = useMemo(
    () => (snapshot ? allWikiPages(snapshot.wiki) : []),
    [snapshot]
  );
  const availablePaths = useMemo(
    () => new Set(allPages.map((page) => page.path)),
    [allPages]
  );
  const selectedPage =
    allPages.find((page) => page.path === selectedPath) ?? null;

  const saveLanguage = async () => {
    if (openwiki || !repoId || !snapshot?.wiki.exists) return;
    setSaving(true);
    setError(null);
    try {
      const next = await workspacesApi.wiki.updateConfig(
        workspace.id,
        repoId,
        language.trim()
      );
      setSnapshot(next);
      setSelectedPath(
        next.wiki.index?.path ?? next.wiki.pages[0]?.path ?? 'index.md'
      );
    } catch (reason) {
      setError(
        reason instanceof Error ? reason.message : 'Unable to update Wiki'
      );
    } finally {
      setSaving(false);
    }
  };

  const handleMarkdownClick = (event: MouseEvent<HTMLDivElement>) => {
    const anchor = (event.target as HTMLElement).closest('a');
    const href = anchor?.getAttribute('href');
    if (!href || !selectedPage) return;
    const target = resolveWikiHref(
      selectedPage.path,
      href,
      availablePaths,
      openwiki
    );
    if (!target) return;
    event.preventDefault();
    setSelectedPath(target);
  };

  return (
    <div className="flex min-h-0 w-full flex-col bg-secondary text-sm">
      <div className="space-y-half border-b p-base">
        <div className="flex items-center justify-between gap-half">
          <span className="rounded bg-panel px-half py-[2px] text-xs text-low">
            Current workspace · {openwiki ? 'openwiki/' : '.llm-wiki/'}
          </span>
          <button
            type="button"
            title="Reload Wiki files"
            aria-label="Reload Wiki files"
            disabled={loading}
            onClick={() => void load()}
            className="rounded p-half text-low hover:bg-panel hover:text-high disabled:opacity-50"
          >
            <ArrowClockwiseIcon className="size-icon-sm" />
          </button>
        </div>
        <select
          aria-label="Wiki repository"
          value={repoId}
          onChange={(event) => {
            setRepoId(event.target.value);
            loadSequence.current += 1;
            setBootstrapOpen(false);
            setSnapshot(null);
            setSelectedPath('index.md');
            setQuery('');
            setError(null);
          }}
          className="w-full rounded border bg-primary px-half py-half text-high"
        >
          {repositoryOptions.map((repo) => (
            <option key={repo.id} value={repo.id}>
              {repo.name}
            </option>
          ))}
        </select>
        <select
          aria-label="Wiki format"
          value={openwiki ? 'openwiki' : 'llm-wiki'}
          onChange={(event) => {
            loadSequence.current += 1;
            setOpenwiki(event.target.value === 'openwiki');
            try {
              sessionStorage.setItem(formatKey, event.target.value);
            } catch {
              // Viewing still works when browser storage is unavailable.
            }
            setBootstrapOpen(false);
            setSnapshot(null);
            setSelectedPath('index.md');
            setQuery('');
            setError(null);
          }}
          className="w-full rounded border bg-primary px-half py-half text-high"
        >
          <option value="llm-wiki">LLM Wiki · .llm-wiki/</option>
          <option value="openwiki">OpenWiki · openwiki/ (read-only)</option>
        </select>
        {openwiki && (
          <p className="text-xs text-low">
            This shows the current worktree, including unpublished changes.
            Initialisation and Sync use repository memory settings.
          </p>
        )}
        {!openwiki && snapshot && !error && repoId && (
          <button
            type="button"
            className="rounded border px-base py-half text-high hover:bg-panel"
            onClick={() => setBootstrapOpen(true)}
          >
            {snapshot.wiki.pages.length
              ? 'Supplement Wiki from code'
              : 'Create Wiki from existing code'}
          </button>
        )}
        {!openwiki && bootstrapOpen && snapshot && !error && (
          <WikiBootstrapDialog
            key={`${workspace.id}:${repoId}`}
            workspaceId={workspace.id}
            repository={
              repos.find((repo) => repo.id === repoId)?.name ??
              workspace.container_ref ??
              'Current direct-folder workspace'
            }
            initialLanguage={snapshot.wiki.config?.output_language ?? 'en'}
            onClose={() => setBootstrapOpen(false)}
          />
        )}
        {snapshot?.wiki.exists && (
          <>
            {!openwiki && (
              <>
                <div className="flex gap-half">
                  <input
                    list="llm-wiki-language-options"
                    aria-label="Wiki output language"
                    value={language}
                    onChange={(event) => setLanguage(event.target.value)}
                    placeholder="BCP 47 language tag"
                    className="min-w-0 flex-1 rounded border bg-primary px-half py-half text-high"
                  />
                  <datalist id="llm-wiki-language-options">
                    {LANGUAGE_OPTIONS.map(([code, label]) => (
                      <option key={code} value={code} label={label} />
                    ))}
                  </datalist>
                  <button
                    type="button"
                    disabled={saving || !language.trim()}
                    onClick={() => void saveLanguage()}
                    className="rounded bg-brand px-base py-half text-white disabled:opacity-50"
                  >
                    Save
                  </button>
                </div>
                <p className="text-xs text-low">
                  Titles and prose use this language. Existing pages are not
                  translated.
                </p>
              </>
            )}
            <input
              type="search"
              aria-label="Search Wiki"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search Wiki"
              className="w-full rounded border bg-primary px-half py-half text-high"
            />
          </>
        )}
        {error && <p className="text-xs text-error">{error}</p>}
      </div>

      {error ? (
        <EmptyState
          text={
            openwiki
              ? 'OpenWiki files are invalid or unavailable. Inspect the reported error; this viewer does not repair or initialise files.'
              : "The repository's .llm-wiki files are invalid or unavailable. Fix the files in the workspace before starting another LLM Wiki task."
          }
        />
      ) : !repoId ? (
        <EmptyState text="This workspace has no repositories." />
      ) : loading && !snapshot ? (
        <EmptyState text="Loading Wiki…" />
      ) : snapshot && !snapshot.wiki.exists ? (
        <EmptyState
          text={
            openwiki
              ? 'No openwiki/ exists in this repository. Use Initialize Wiki in repository memory settings.'
              : 'No .llm-wiki exists in this repository. It will be initialised automatically before an LLM Wiki-enabled agent run starts.'
          }
        />
      ) : (
        <div className="grid min-h-[320px] flex-1 grid-cols-[minmax(110px,0.34fr)_minmax(0,1fr)] overflow-hidden">
          <nav
            className="overflow-y-auto border-r p-half"
            aria-label="Wiki pages"
          >
            {pages.map((page) => (
              <button
                type="button"
                key={page.path}
                title={page.path}
                onClick={() => setSelectedPath(page.path)}
                className={`mb-[2px] w-full rounded px-half py-half text-left text-xs ${
                  selectedPath === page.path
                    ? 'bg-panel text-high'
                    : 'text-normal hover:bg-panel/70'
                }`}
              >
                <span className="block truncate">{pageTitle(page)}</span>
              </button>
            ))}
            {pages.length === 0 && (
              <p className="p-half text-xs text-low">No matching pages.</p>
            )}
          </nav>
          <article className="min-w-0 overflow-y-auto p-base">
            {selectedPage ? (
              <>
                {selectedPage.metadata && (
                  <PageMetadata
                    page={selectedPage}
                    sourceLinks={snapshot?.source_links ?? []}
                  />
                )}
                <div onClick={handleMarkdownClick}>
                  <MarkdownPreview
                    content={expandWikiLinks(selectedPage.content)}
                    theme={resolvedTheme}
                  />
                </div>
              </>
            ) : (
              <EmptyState text="Select a Wiki page." />
            )}
          </article>
        </div>
      )}
    </div>
  );
}

function EmptyState({ text }: { text: string }) {
  return (
    <div className="flex min-h-[200px] flex-1 flex-col items-center justify-center gap-half p-base text-center text-low">
      <BookOpenIcon className="size-icon-lg" />
      <p className="max-w-xs text-xs">{text}</p>
    </div>
  );
}

function PageMetadata({
  page,
  sourceLinks,
}: {
  page: WikiPage;
  sourceLinks: WorkspaceWikiSnapshot['source_links'];
}) {
  const metadata = page.metadata;
  if (!metadata) return null;
  const links = new Map(sourceLinks.map((link) => [link.source, link]));
  return (
    <div className="mb-base space-y-[2px] rounded border bg-panel/50 p-half text-xs text-low">
      <p className="font-medium text-high">{metadata.title}</p>
      <p>{metadata.summary}</p>
      {metadata.language && <p>Language: {metadata.language}</p>}
      {metadata.tags.length > 0 && <p>Tags: {metadata.tags.join(', ')}</p>}
      {metadata.sources.length > 0 && (
        <p>
          Sources:{' '}
          {metadata.sources.map((source, index) => {
            const link = links.get(source);
            return (
              <span key={source}>
                {index > 0 && ', '}
                {link ? (
                  <a
                    className="text-brand hover:underline"
                    href={`/projects/${link.project_id}/issues/${link.issue_id}`}
                  >
                    {source}
                  </a>
                ) : (
                  source
                )}
              </span>
            );
          })}
        </p>
      )}
      <p>
        {metadata.updated && `Updated: ${metadata.updated} · `}
        {page.path}
      </p>
    </div>
  );
}
