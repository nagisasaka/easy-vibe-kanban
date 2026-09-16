import type { WorkspaceWikiSnapshot } from 'shared/types';
import { allWikiPages, resolveWikiHref } from './wikiNavigation';

interface WikiViewerApi {
  snapshot: (
    workspaceId: string,
    repoId: string,
    openwiki: boolean
  ) => Promise<WorkspaceWikiSnapshot>;
  updateConfig: (
    workspaceId: string,
    repoId: string,
    language: string
  ) => Promise<WorkspaceWikiSnapshot>;
}

export interface WikiViewerState {
  repoId: string;
  openwiki: boolean;
  snapshot: WorkspaceWikiSnapshot | null;
  selectedPath: string;
  query: string;
  language: string;
  loading: boolean;
  saving: boolean;
  bootstrapOpen: boolean;
  error: string | null;
  collapsedDirectories: string[];
}

function readFormat(workspaceId: string) {
  try {
    return (
      sessionStorage.getItem(`evk-wiki-viewer-format:${workspaceId}`) ===
      'openwiki'
    );
  } catch {
    return false;
  }
}

/** One controller owns both surfaces. Async responses are scoped, never shared
 * across repositories/formats, and changing UI panels does not discard selection. */
export function createWikiViewerController(
  workspaceId: string,
  initialRepoId: string,
  api: WikiViewerApi
) {
  let state: WikiViewerState = {
    repoId: initialRepoId,
    openwiki: readFormat(workspaceId),
    snapshot: null,
    selectedPath: 'index.md',
    query: '',
    language: 'en',
    loading: false,
    saving: false,
    bootstrapOpen: false,
    error: null,
    collapsedDirectories: [],
  };
  let enabled = false;
  let sequence = 0;
  const listeners = new Set<() => void>();
  const scrollPositions = new Map<string, number>();
  const update = (patch: Partial<WikiViewerState>) => {
    state = { ...state, ...patch };
    listeners.forEach((listener) => listener());
  };
  const acceptSnapshot = (snapshot: WorkspaceWikiSnapshot) => {
    if (
      snapshot.workspace_id !== workspaceId ||
      snapshot.repo_id !== state.repoId
    ) {
      throw new Error(
        'Wiki response does not match the selected workspace and repository'
      );
    }
    const paths = new Set(allWikiPages(snapshot.wiki).map((page) => page.path));
    update({
      snapshot,
      language: snapshot.wiki.config?.output_language ?? 'en',
      selectedPath: paths.has(state.selectedPath)
        ? state.selectedPath
        : (snapshot.wiki.index?.path ??
          snapshot.wiki.pages[0]?.path ??
          'index.md'),
    });
  };
  const reload = async () => {
    const request = ++sequence;
    if (!state.repoId) {
      update({ snapshot: null, loading: false, saving: false });
      return;
    }
    update({ loading: true, saving: false, error: null });
    try {
      const next = await api.snapshot(
        workspaceId,
        state.repoId,
        state.openwiki
      );
      if (request !== sequence) return;
      acceptSnapshot(next);
    } catch (reason) {
      if (request !== sequence) return;
      update({
        snapshot: null,
        error: reason instanceof Error ? reason.message : 'Unable to load Wiki',
      });
    } finally {
      if (request === sequence) update({ loading: false });
    }
  };
  const changeScope = (patch: Partial<WikiViewerState>) => {
    sequence += 1;
    scrollPositions.clear();
    update({
      ...patch,
      snapshot: null,
      selectedPath: 'index.md',
      query: '',
      language: 'en',
      loading: false,
      saving: false,
      bootstrapOpen: false,
      error: null,
      collapsedDirectories: [],
    });
    if (enabled) void reload();
  };

  return {
    getSnapshot: () => state,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    setEnabled(value: boolean) {
      if (enabled === value) return;
      enabled = value;
      if (!value) {
        sequence += 1;
        update({ loading: false, saving: false });
      } else if (!state.snapshot) {
        void reload();
      }
    },
    selectRepository(repoId: string) {
      if (repoId !== state.repoId) changeScope({ repoId });
    },
    selectFormat(openwiki: boolean) {
      if (openwiki === state.openwiki) return;
      try {
        sessionStorage.setItem(
          `evk-wiki-viewer-format:${workspaceId}`,
          openwiki ? 'openwiki' : 'llm-wiki'
        );
      } catch {
        /* Viewing also works without browser storage. */
      }
      changeScope({ openwiki });
    },
    selectPage(path: string) {
      if (
        state.snapshot &&
        allWikiPages(state.snapshot.wiki).some((page) => page.path === path)
      ) {
        update({ selectedPath: path });
      }
    },
    followLink(href: string) {
      if (!state.snapshot) return false;
      const paths = new Set(
        allWikiPages(state.snapshot.wiki).map((page) => page.path)
      );
      const target = resolveWikiHref(
        state.selectedPath,
        href,
        paths,
        state.openwiki
      );
      if (!target) return false;
      update({ selectedPath: target });
      return true;
    },
    setQuery(query: string) {
      update({ query });
    },
    setLanguage(language: string) {
      update({ language });
    },
    setBootstrapOpen(bootstrapOpen: boolean) {
      update({ bootstrapOpen });
    },
    toggleDirectory(path: string) {
      update({
        collapsedDirectories: state.collapsedDirectories.includes(path)
          ? state.collapsedDirectories.filter((value) => value !== path)
          : [...state.collapsedDirectories, path],
      });
    },
    rememberScroll(path: string, top: number) {
      scrollPositions.set(path, top);
    },
    scrollPosition(path: string) {
      return scrollPositions.get(path) ?? 0;
    },
    reload,
    async saveLanguage() {
      if (
        state.openwiki ||
        !state.repoId ||
        !state.snapshot?.wiki.exists ||
        !state.language.trim()
      )
        return;
      const request = ++sequence;
      update({ saving: true, loading: false, error: null });
      try {
        const next = await api.updateConfig(
          workspaceId,
          state.repoId,
          state.language.trim()
        );
        if (request !== sequence) return;
        acceptSnapshot(next);
      } catch (reason) {
        if (request !== sequence) return;
        update({
          error:
            reason instanceof Error ? reason.message : 'Unable to update Wiki',
        });
      } finally {
        if (request === sequence) update({ saving: false });
      }
    },
  };
}

export type WikiViewerController = ReturnType<
  typeof createWikiViewerController
>;
