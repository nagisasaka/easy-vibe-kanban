import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  type MouseEvent,
  type ReactNode,
} from 'react';
import type {
  RepoWithTargetBranch,
  WikiPage,
  Workspace,
  WorkspaceWikiSnapshot,
} from 'shared/types';
import {
  ArrowClockwiseIcon,
  BookOpenIcon,
  CaretDownIcon,
  CaretRightIcon,
  FolderIcon,
} from '@phosphor-icons/react';
import { MarkdownPreview } from '@/shared/components/MarkdownPreview';
import { getResolvedTheme, useTheme } from '@/shared/hooks/useTheme';
import { WikiBootstrapDialog } from './WikiBootstrapDialog';
import {
  WorkspaceWikiProvider,
  useWorkspaceWiki,
} from './WorkspaceWikiProvider';
import {
  allWikiPages,
  activateWikiLink,
  buildWikiTree,
  expandWikiLinks,
  pageTitle,
  resolveWikiHref,
  searchWikiPages,
  type WikiTreeNode,
} from '../model/wikiNavigation';

const useScrollLayoutEffect =
  typeof window === 'undefined' ? useEffect : useLayoutEffect;

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

/** Compatibility surface for callers outside the workspace layout. */
export function WorkspaceWikiPanel({
  workspace,
  repos,
}: {
  workspace: Workspace;
  repos: RepoWithTargetBranch[];
}) {
  return (
    <WorkspaceWikiProvider workspace={workspace} repos={repos}>
      <div className="flex h-full min-h-0 w-full">
        <div className="w-64 shrink-0 overflow-auto">
          <WorkspaceWikiNavigation />
        </div>
        <WorkspaceWikiArticle />
      </div>
    </WorkspaceWikiProvider>
  );
}

/** Sidebar controls and the public page tree; the article has its own surface. */
export function WorkspaceWikiNavigation({
  onSelectPage,
  onReturnToChat,
  showReload = false,
}: {
  onSelectPage?: () => void;
  onReturnToChat?: () => void;
  showReload?: boolean;
}) {
  const wiki = useWorkspaceWiki();
  const {
    workspace,
    repositoryOptions,
    repoId,
    openwiki,
    snapshot,
    query,
    language,
    saving,
    error,
    bootstrapOpen,
    controller,
  } = wiki;
  const pages = useMemo(
    () => (snapshot ? searchWikiPages(snapshot.wiki, query) : []),
    [snapshot, query]
  );
  const tree = useMemo(() => buildWikiTree(pages), [pages]);
  return (
    <div className="flex h-full min-h-0 w-full flex-1 flex-col bg-secondary text-base">
      <div className="shrink-0 space-y-half border-b p-base">
        <div className="flex items-center justify-between gap-half">
          <p className="text-sm text-low">
            Current workspace · {openwiki ? 'openwiki/' : '.llm-wiki/'}
          </p>
          {showReload && (
            <button
              type="button"
              aria-label="Reload Wiki files"
              title="Reload Wiki files"
              disabled={wiki.loading || !repoId}
              onClick={() => void controller.reload()}
              className="shrink-0 rounded p-half text-low hover:bg-panel hover:text-high disabled:opacity-50"
            >
              <ArrowClockwiseIcon className="size-icon-base" />
            </button>
          )}
        </div>
        <select
          aria-label="Wiki repository"
          value={repoId}
          onChange={(event) => controller.selectRepository(event.target.value)}
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
          onChange={(event) =>
            controller.selectFormat(event.target.value === 'openwiki')
          }
          className="w-full rounded border bg-primary px-half py-half text-high"
        >
          <option value="llm-wiki">LLM Wiki · .llm-wiki/</option>
          <option value="openwiki">OpenWiki · openwiki/ (read-only)</option>
        </select>
        {openwiki && (
          <p className="text-sm text-low">
            This shows the current worktree, including unpublished changes.
            Initialisation and Sync use repository memory settings.
          </p>
        )}
        {!openwiki && snapshot && !error && repoId && (
          <button
            type="button"
            className="rounded border px-base py-half text-high hover:bg-panel"
            onClick={() => controller.setBootstrapOpen(true)}
          >
            {snapshot.wiki.pages.length
              ? 'Supplement Wiki from code'
              : 'Create Wiki from existing code'}
          </button>
        )}
        {!openwiki && bootstrapOpen && workspace && snapshot && !error && (
          <WikiBootstrapDialog
            key={`${workspace.id}:${repoId}`}
            workspaceId={workspace.id}
            repository={
              repositoryOptions.find((repo) => repo.id === repoId)?.name ??
              'Current direct-folder workspace'
            }
            initialLanguage={snapshot.wiki.config?.output_language ?? 'en'}
            onClose={() => controller.setBootstrapOpen(false)}
            onReturnToChat={onReturnToChat}
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
                    onChange={(event) =>
                      controller.setLanguage(event.target.value)
                    }
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
                    onClick={() => void controller.saveLanguage()}
                    className="rounded bg-brand px-base py-half text-white disabled:opacity-50"
                  >
                    Save
                  </button>
                </div>
                <p className="text-sm text-low">
                  Titles and prose use this language. Existing pages are not
                  translated.
                </p>
              </>
            )}
            <input
              type="search"
              aria-label="Search Wiki"
              value={query}
              onChange={(event) => controller.setQuery(event.target.value)}
              placeholder="Search Wiki"
              className="w-full rounded border bg-primary px-half py-half text-high"
            />
          </>
        )}
        {error && (
          <p role="status" className="text-sm text-error">
            {error}
          </p>
        )}
      </div>
      <nav
        className="min-h-0 flex-1 overflow-y-auto p-half"
        aria-label="Wiki pages"
      >
        <WikiPageTree nodes={tree} onSelectPage={onSelectPage} />
        {snapshot?.wiki.exists && pages.length === 0 && (
          <p className="p-half text-sm text-low">No matching pages.</p>
        )}
        {!snapshot && wiki.loading && (
          <p className="p-half text-sm text-low">Loading Wiki…</p>
        )}
        {snapshot && !snapshot.wiki.exists && (
          <p className="p-half text-sm text-low">
            No Wiki pages in this repository.
          </p>
        )}
      </nav>
    </div>
  );
}

function WikiPageTree({
  nodes,
  onSelectPage,
}: {
  nodes: WikiTreeNode[];
  onSelectPage?: () => void;
}) {
  const { selectedPath, collapsedDirectories, query, controller } =
    useWorkspaceWiki();
  return (
    <ul className="space-y-[2px]">
      {nodes.map((node) => {
        if (node.kind === 'page')
          return (
            <li key={node.path}>
              <button
                type="button"
                title={node.path}
                aria-current={selectedPath === node.path ? 'page' : undefined}
                onClick={() => {
                  controller.selectPage(node.path);
                  onSelectPage?.();
                }}
                className={`w-full rounded px-half py-half text-left text-base ${selectedPath === node.path ? 'bg-panel text-high' : 'text-normal hover:bg-panel/70'}`}
              >
                <span className="block break-words">
                  {pageTitle(node.page)}
                </span>
              </button>
            </li>
          );
        const expanded =
          Boolean(query.trim()) || !collapsedDirectories.includes(node.path);
        return (
          <li key={node.path}>
            <button
              type="button"
              title={node.path}
              aria-label={`Directory ${node.path}`}
              aria-expanded={expanded}
              disabled={Boolean(query.trim())}
              onClick={() => controller.toggleDirectory(node.path)}
              className="flex w-full items-center gap-half rounded px-half py-half text-left text-base text-low hover:bg-panel/70"
            >
              {expanded ? (
                <CaretDownIcon className="size-icon-xs shrink-0" />
              ) : (
                <CaretRightIcon className="size-icon-xs shrink-0" />
              )}
              <FolderIcon className="size-icon-sm shrink-0" />
              <span className="min-w-0 break-words">{node.name}</span>
            </button>
            {expanded && (
              <div className="ml-base border-l pl-half">
                <WikiPageTree
                  nodes={node.children}
                  onSelectPage={onSelectPage}
                />
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

/** Full-height read-only body; layout/focus actions are supplied by the parent. */
export function WorkspaceWikiArticle({ actions }: { actions?: ReactNode }) {
  const { theme } = useTheme();
  const {
    workspace,
    repositoryOptions,
    repoId,
    openwiki,
    snapshot,
    selectedPath,
    loading,
    error,
    controller,
  } = useWorkspaceWiki();
  const pages = useMemo(
    () => (snapshot ? allWikiPages(snapshot.wiki) : []),
    [snapshot]
  );
  const selectedPage = pages.find((page) => page.path === selectedPath) ?? null;
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrollKey = JSON.stringify([
    workspace?.id,
    repoId,
    openwiki,
    selectedPath,
  ]);
  useScrollLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    element.scrollTop = controller.scrollPosition(scrollKey);
  }, [controller, scrollKey]);

  const handleMarkdownClick = (event: MouseEvent<HTMLDivElement>) => {
    const href = (event.target as HTMLElement)
      .closest('a')
      ?.getAttribute('href');
    if (!href || !selectedPage) return;
    activateWikiLink(
      resolveWikiHref(
        selectedPath,
        href,
        new Set(pages.map((page) => page.path)),
        openwiki
      ),
      event,
      (path) => controller.selectPage(path)
    );
  };
  const missing = !workspace
    ? 'Select a workspace to read its Wiki.'
    : error
      ? openwiki
        ? 'OpenWiki files are invalid or unavailable. Inspect the reported error; this viewer does not repair or initialise files.'
        : "The repository's .llm-wiki files are invalid or unavailable. Fix the files in the workspace before starting another LLM Wiki task."
      : !repoId
        ? 'This workspace has no repositories.'
        : loading && !snapshot
          ? 'Loading Wiki…'
          : snapshot && !snapshot.wiki.exists
            ? openwiki
              ? 'No openwiki/ exists in this repository. Use Initialize Wiki in repository memory settings.'
              : 'No .llm-wiki exists in this repository. It will be initialised automatically before an LLM Wiki-enabled agent run starts.'
            : !selectedPage
              ? 'Select a Wiki page.'
              : null;
  return (
    <section
      aria-label="Wiki article"
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-primary"
    >
      <header className="flex shrink-0 flex-wrap items-start justify-between gap-base border-b bg-secondary p-base">
        <div className="min-w-0 flex-1 basis-[180px]">
          <p className="text-sm text-low">
            Current workspace · {openwiki ? 'openwiki/' : '.llm-wiki/'} ·
            Read-only
          </p>
          <p className="break-words text-base text-high">
            {snapshot?.repo_display_name ??
              repositoryOptions.find((repo) => repo.id === repoId)?.name ??
              'Wiki'}
            {workspace?.name && ` · ${workspace.name}`}
          </p>
          {workspace?.branch && (
            <p className="break-all text-sm text-low">
              Workspace branch: {workspace.branch}
            </p>
          )}
          {selectedPage && (
            <p className="break-all text-sm text-low">{selectedPage.path}</p>
          )}
        </div>
        <div className="flex max-w-full shrink-0 flex-wrap items-center gap-half">
          <button
            type="button"
            title="Reload Wiki files"
            aria-label="Reload Wiki files"
            disabled={loading || !repoId}
            onClick={() => void controller.reload()}
            className="rounded p-half text-low hover:bg-panel hover:text-high disabled:opacity-50"
          >
            <ArrowClockwiseIcon className="size-icon-base" />
          </button>
          {actions}
        </div>
      </header>
      {error && (
        <p role="status" className="border-b p-base text-base text-error">
          {error}
        </p>
      )}
      <div
        ref={scrollRef}
        data-wiki-article-scroll="true"
        className="min-h-0 flex-1 overflow-auto px-double py-base"
        onScroll={(event) =>
          controller.rememberScroll(scrollKey, event.currentTarget.scrollTop)
        }
      >
        {missing ? (
          <EmptyState text={missing} />
        ) : (
          selectedPage && (
            <article className="mx-auto w-full max-w-[880px] min-w-0">
              {selectedPage.metadata && (
                <PageMetadata
                  page={selectedPage}
                  sourceLinks={snapshot?.source_links ?? []}
                />
              )}
              <div onClickCapture={handleMarkdownClick}>
                <MarkdownPreview
                  content={expandWikiLinks(selectedPage.content)}
                  theme={getResolvedTheme(theme)}
                  className="min-w-0 break-words [&_p]:text-[14px] [&_li]:text-[14px] [&_table]:text-[13px] [&_pre]:max-w-full [&_pre]:text-[12px] [&_h1]:text-[24px] [&_h2]:text-[20px] [&_h3]:text-[17px]"
                />
              </div>
            </article>
          )
        )}
      </div>
    </section>
  );
}

function EmptyState({ text }: { text: string }) {
  return (
    <div className="flex min-h-[200px] flex-1 flex-col items-center justify-center gap-half p-base text-center text-low">
      <BookOpenIcon className="size-icon-lg" />
      <p className="max-w-md text-base">{text}</p>
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
    <div className="mb-double space-y-half rounded border bg-panel/50 p-base text-base text-low">
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
      {metadata.updated && <p>Updated: {metadata.updated}</p>}
    </div>
  );
}
