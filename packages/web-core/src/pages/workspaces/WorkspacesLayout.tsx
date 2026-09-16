import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  Group,
  Layout,
  Panel,
  Separator,
  useGroupCallbackRef,
} from 'react-resizable-panels';
import type { CreateModeInitialState } from '@/shared/types/createMode';
import { useWorkspaceContext } from '@/shared/hooks/useWorkspaceContext';
import { usePageTitle } from '@/shared/hooks/usePageTitle';
import { useIsMobile } from '@/shared/hooks/useIsMobile';
import { useMobileActiveTab } from '@/shared/stores/useUiPreferencesStore';
import { cn } from '@/shared/lib/utils';
import { CreateModeProvider } from '@/features/create-mode/model/CreateModeProvider';
import {
  consumeCreateModeSeedState,
  getCreateModeSeedVersion,
  subscribeCreateModeSeedState,
} from '@/features/create-mode/model/createModeSeedStore';
import { ReviewProvider } from '@/shared/hooks/ReviewProvider';
import { ChangesViewProvider } from '@/shared/hooks/ChangesViewProvider';
import { WorkspacesSidebarContainer } from './WorkspacesSidebarContainer';
import { LogsContentContainer } from './LogsContentContainer';
import {
  WorkspacesMainContainer,
  type WorkspacesMainContainerHandle,
} from './WorkspacesMainContainer';
import { RightSidebar } from './RightSidebar';
import { PreservedChatPanel } from './PreservedChatPanel';
import {
  WorkspaceWikiNavigation,
  WorkspaceWikiArticle,
} from '@/features/wiki/ui/WorkspaceWikiPanel';
import { WorkspaceWikiProvider } from '@/features/wiki/ui/WorkspaceWikiProvider';
import { ChangesPanelContainer } from './ChangesPanelContainer';
import { CreateChatBoxContainer } from '@/shared/components/CreateChatBoxContainer';
import { PreviewBrowserContainer } from './PreviewBrowserContainer';
import { WorkspaceFilesSurfaceContainer } from './WorkspaceFilesSurfaceContainer';
import { WorkspacesGuideDialog } from '@/shared/dialogs/shared/WorkspacesGuideDialog';
import { useUserSystem } from '@/shared/hooks/useUserSystem';
import {
  WorkspaceFilePreviewActionsProvider,
  WorkspaceFilesSelectionProvider,
  useWorkspaceFilesSelection,
  type WorkspaceFilePreviewTarget,
} from '@/features/workspace-files';

import {
  PERSIST_KEYS,
  usePaneSize,
  useWorkspacePanelState,
  useUiPreferencesStore,
  RIGHT_MAIN_PANEL_MODES,
  type RightMainPanelMode,
} from '@/shared/stores/useUiPreferencesStore';
import { useAppNavigation } from '@/shared/hooks/useAppNavigation';

const WORKSPACES_GUIDE_ID = 'workspaces-guide';
const MAIN_PANEL_MIN_SIZE = '12%';

export function WorkspacesLayout() {
  const appNavigation = useAppNavigation();
  const {
    workspaceId,
    workspace: selectedWorkspace,
    isLoading,
    isCreateMode,
    selectedSession,
    selectedSessionId,
    sessions,
    isSessionsLoading,
    selectSession,
    repos,
    isNewSessionMode,
    startNewSession,
  } = useWorkspaceContext();

  const { t } = useTranslation('common');
  usePageTitle(
    isCreateMode ? t('workspaces.newWorkspace') : selectedWorkspace?.name
  );

  const seedVersion = useSyncExternalStore(
    subscribeCreateModeSeedState,
    getCreateModeSeedVersion,
    getCreateModeSeedVersion
  );
  const consumedSeedVersionRef = useRef(0);
  const [createModeSeed, setCreateModeSeed] = useState<{
    version: number;
    state: CreateModeInitialState | null;
  }>({
    version: 0,
    state: null,
  });

  useEffect(() => {
    if (!isCreateMode) {
      consumedSeedVersionRef.current = 0;
      setCreateModeSeed((current) =>
        current.version === 0 && current.state === null
          ? current
          : { version: 0, state: null }
      );
      return;
    }

    if (seedVersion === 0 || seedVersion === consumedSeedVersionRef.current) {
      return;
    }

    consumedSeedVersionRef.current = seedVersion;
    setCreateModeSeed({
      version: seedVersion,
      state: consumeCreateModeSeedState(),
    });
  }, [isCreateMode, seedVersion]);

  const createModeProviderKey =
    createModeSeed.version > 0
      ? `create-mode-seed-${createModeSeed.version}`
      : 'create-mode-seed-default';

  const isMobile = useIsMobile();
  // Use workspace-specific panel state (pass undefined when in create mode).
  const {
    isLeftSidebarVisible,
    isLeftMainPanelVisible,
    isRightSidebarVisible,
    rightMainPanelMode,
    setLeftSidebarVisible,
    setLeftMainPanelVisible,
    setRightMainPanelMode,
  } = useWorkspacePanelState(isCreateMode ? undefined : workspaceId);
  const [mobileTab, setMobileTab] = useMobileActiveTab();
  const closeWiki = useUiPreferencesStore((s) => s.closeWiki);
  const [wikiMobileNavigation, setWikiMobileNavigation] = useState(true);
  useEffect(() => setWikiMobileNavigation(true), [workspaceId]);
  const wikiActive = rightMainPanelMode === RIGHT_MAIN_PANEL_MODES.WIKI;
  const wikiButtonClass =
    'rounded-sm border px-base py-half text-normal hover:bg-panel';
  const wikiActions = (
    <>
      {isMobile ? (
        <button
          type="button"
          className={wikiButtonClass}
          onClick={() => setWikiMobileNavigation(true)}
        >
          Wiki pages
        </button>
      ) : (
        <button
          type="button"
          className={wikiButtonClass}
          onClick={() => setLeftMainPanelVisible(!isLeftMainPanelVisible)}
        >
          {isLeftMainPanelVisible ? 'Focus Wiki' : 'Show chat alongside'}
        </button>
      )}
      <button
        type="button"
        className={wikiButtonClass}
        onClick={() => {
          closeWiki(workspaceId);
          if (isMobile) setMobileTab('chat');
        }}
      >
        Return to workspace
      </button>
    </>
  );
  const mainContainerRef = useRef<WorkspacesMainContainerHandle>(null);

  const handleScrollToBottom = useCallback(
    (behavior: 'auto' | 'smooth' = 'smooth') => {
      mainContainerRef.current?.scrollToBottom(behavior);
    },
    []
  );

  const handleWorkspaceCreated = useCallback(
    (workspaceId: string) => {
      appNavigation.goToWorkspace(workspaceId);
    },
    [appNavigation]
  );

  const {
    config,
    updateAndSaveConfig,
    loading: configLoading,
  } = useUserSystem();
  const hasAutoShownWorkspacesGuide = useRef(false);

  // Auto-show Workspaces Guide on first visit
  useEffect(() => {
    if (hasAutoShownWorkspacesGuide.current) return;
    if (configLoading || !config) return;

    const seenFeatures = config.showcases?.seen_features ?? [];
    if (seenFeatures.includes(WORKSPACES_GUIDE_ID)) return;

    hasAutoShownWorkspacesGuide.current = true;

    void updateAndSaveConfig({
      showcases: { seen_features: [...seenFeatures, WORKSPACES_GUIDE_ID] },
    });
    WorkspacesGuideDialog.show().finally(() => WorkspacesGuideDialog.hide());
  }, [configLoading, config, updateAndSaveConfig]);

  // Ensure left panels visible when right main panel hidden
  useEffect(() => {
    if (rightMainPanelMode === null) {
      setLeftSidebarVisible(true);
      if (!isLeftMainPanelVisible) setLeftMainPanelVisible(true);
    }
  }, [
    isLeftMainPanelVisible,
    rightMainPanelMode,
    setLeftSidebarVisible,
    setLeftMainPanelVisible,
  ]);

  const [rightMainPanelSize, setRightMainPanelSize] = usePaneSize(
    PERSIST_KEYS.rightMainPanel,
    50
  );

  const splitSize =
    typeof rightMainPanelSize === 'number' &&
    Number.isFinite(rightMainPanelSize) &&
    rightMainPanelSize >= 12 &&
    rightMainPanelSize <= 88
      ? rightMainPanelSize
      : 50;
  const defaultLayout: Layout =
    rightMainPanelMode === null
      ? { 'left-main': 100, 'right-main': 0 }
      : {
          'left-main': isLeftMainPanelVisible ? 100 - splitSize : 0,
          'right-main': isLeftMainPanelVisible ? splitSize : 100,
        };

  const [mainGroup, setMainGroup] = useGroupCallbackRef();
  useEffect(() => {
    if (!mainGroup || isMobile) return;
    // Keep both Panels registered; only change their allocated space after
    // Group registration. Chat state and geometry survive focus/return.
    const frame = requestAnimationFrame(() => {
      mainGroup.setLayout(
        rightMainPanelMode === null
          ? { 'left-main': 100, 'right-main': 0 }
          : {
              'left-main': isLeftMainPanelVisible ? 100 - splitSize : 0,
              'right-main': isLeftMainPanelVisible ? splitSize : 100,
            }
      );
    });
    return () => cancelAnimationFrame(frame);
  }, [
    mainGroup,
    isMobile,
    rightMainPanelMode,
    isLeftMainPanelVisible,
    splitSize,
  ]);

  const layoutTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (layoutTimerRef.current) clearTimeout(layoutTimerRef.current);
    };
  }, []);

  const onLayoutChange = useCallback(
    (layout: Layout) => {
      const size = layout['right-main'];
      // Ignore collapsed and transitional layouts: those are not a user's
      // preferred chat/article split and may arrive during a focus switch.
      if (
        isLeftMainPanelVisible &&
        rightMainPanelMode !== null &&
        Number.isFinite(size) &&
        size >= 12 &&
        size <= 88
      ) {
        if (layoutTimerRef.current) clearTimeout(layoutTimerRef.current);
        layoutTimerRef.current = setTimeout(() => {
          setRightMainPanelSize(size);
        }, 150);
      }
    },
    [isLeftMainPanelVisible, rightMainPanelMode, setRightMainPanelSize]
  );

  // ── Mobile layout ──────────────────────────────────────────────────
  // Uses `hidden` CSS class (NOT conditional rendering) to preserve
  // WebSocket connections and scroll positions across tab switches.
  if (isMobile) {
    const mobileContent = (
      <ReviewProvider workspaceId={selectedWorkspace?.id}>
        <ChangesViewProvider workspaceId={selectedWorkspace?.id}>
          <WorkspaceFilesSelectionProvider>
            <MainWorkspaceFilePreviewActionsBridge
              workspaceId={selectedWorkspace?.id}
              setRightMainPanelMode={setRightMainPanelMode}
            >
              <div className="flex flex-col h-full min-h-0">
                {/* Workspaces tab */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'workspaces' && 'hidden'
                  )}
                >
                  <WorkspacesSidebarContainer
                    onScrollToBottom={handleScrollToBottom}
                  />
                </div>

                {/* Chat tab */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'chat' && 'hidden'
                  )}
                >
                  {isCreateMode ? (
                    <CreateChatBoxContainer
                      onWorkspaceCreated={handleWorkspaceCreated}
                    />
                  ) : (
                    <WorkspacesMainContainer
                      ref={mainContainerRef}
                      selectedWorkspace={selectedWorkspace ?? null}
                      selectedSession={selectedSession}
                      selectedSessionId={selectedSessionId}
                      sessions={sessions}
                      repos={repos}
                      onSelectSession={selectSession}
                      isLoading={isLoading}
                      isSessionsLoading={isSessionsLoading}
                      isNewSessionMode={isNewSessionMode}
                      onStartNewSession={startNewSession}
                    />
                  )}
                </div>

                {/* Files tab */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'files' && 'hidden'
                  )}
                >
                  {selectedWorkspace?.id && !isCreateMode && (
                    <WorkspaceFilesSurfaceContainer
                      workspaceId={selectedWorkspace.id}
                      mobile
                    />
                  )}
                </div>

                {/* Wiki and chat stay mounted when changing mobile tabs. */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'wiki' && 'hidden'
                  )}
                >
                  <div
                    className={cn(
                      'h-full flex flex-col',
                      !wikiMobileNavigation && 'hidden'
                    )}
                  >
                    <div className="p-base border-b text-high">Wiki pages</div>
                    <WorkspaceWikiNavigation
                      showReload
                      onReturnToChat={() => {
                        closeWiki(workspaceId);
                        setMobileTab('chat');
                      }}
                      onSelectPage={() => setWikiMobileNavigation(false)}
                    />
                  </div>
                  <div
                    className={cn('h-full', wikiMobileNavigation && 'hidden')}
                  >
                    <WorkspaceWikiArticle actions={wikiActions} />
                  </div>
                </div>

                {/* Changes tab */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'changes' && 'hidden'
                  )}
                >
                  {selectedWorkspace?.id && (
                    <ChangesPanelContainer
                      className=""
                      workspaceId={selectedWorkspace.id}
                    />
                  )}
                </div>

                {/* Logs tab */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'logs' && 'hidden'
                  )}
                >
                  <LogsContentContainer className="" />
                </div>

                {/* Preview tab */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'preview' && 'hidden'
                  )}
                >
                  {selectedWorkspace?.id && (
                    <PreviewBrowserContainer
                      workspaceId={selectedWorkspace.id}
                      className=""
                    />
                  )}
                </div>

                {/* Git tab */}
                <div
                  className={cn(
                    'flex-1 min-h-0 overflow-hidden',
                    mobileTab !== 'git' && 'hidden'
                  )}
                >
                  {selectedWorkspace && !isCreateMode && (
                    <RightSidebar
                      rightMainPanelMode={
                        wikiActive ? null : rightMainPanelMode
                      }
                      selectedWorkspace={selectedWorkspace}
                      repos={repos}
                    />
                  )}
                </div>
              </div>
            </MainWorkspaceFilePreviewActionsBridge>
          </WorkspaceFilesSelectionProvider>
        </ChangesViewProvider>
      </ReviewProvider>
    );

    return (
      <WorkspaceWikiProvider
        workspace={selectedWorkspace}
        repos={repos}
        enabled={!isCreateMode && mobileTab === 'wiki'}
      >
        <div className="flex flex-1 min-h-0 h-full">
          <div className="flex-1 min-w-0 h-full">
            {isCreateMode ? (
              <CreateModeProvider
                key={createModeProviderKey}
                initialState={createModeSeed.state}
              >
                {mobileContent}
              </CreateModeProvider>
            ) : (
              mobileContent
            )}
          </div>
        </div>
      </WorkspaceWikiProvider>
    );
  }

  const mainContent = (
    <ReviewProvider workspaceId={selectedWorkspace?.id}>
      <ChangesViewProvider workspaceId={selectedWorkspace?.id}>
        <WorkspaceFilesSelectionProvider>
          <MainWorkspaceFilePreviewActionsBridge
            workspaceId={selectedWorkspace?.id}
            setRightMainPanelMode={setRightMainPanelMode}
          >
            <div className="flex h-full">
              <Group
                groupRef={setMainGroup}
                orientation="horizontal"
                className="flex-1 min-w-0 h-full"
                defaultLayout={defaultLayout}
                onLayoutChange={onLayoutChange}
              >
                <PreservedChatPanel visible={isLeftMainPanelVisible}>
                  {isCreateMode ? (
                    <CreateChatBoxContainer
                      onWorkspaceCreated={handleWorkspaceCreated}
                    />
                  ) : (
                    <WorkspacesMainContainer
                      ref={mainContainerRef}
                      selectedWorkspace={selectedWorkspace ?? null}
                      selectedSession={selectedSession}
                      selectedSessionId={selectedSessionId}
                      sessions={sessions}
                      repos={repos}
                      onSelectSession={selectSession}
                      isLoading={isLoading}
                      isSessionsLoading={isSessionsLoading}
                      isNewSessionMode={isNewSessionMode}
                      onStartNewSession={startNewSession}
                    />
                  )}
                </PreservedChatPanel>

                <Separator
                  id="main-separator"
                  aria-hidden={
                    !isLeftMainPanelVisible || rightMainPanelMode === null
                  }
                  tabIndex={
                    isLeftMainPanelVisible && rightMainPanelMode !== null
                      ? 0
                      : -1
                  }
                  className={cn(
                    'bg-transparent hover:bg-brand/50 transition-colors cursor-col-resize',
                    isLeftMainPanelVisible && rightMainPanelMode !== null
                      ? 'w-1'
                      : 'w-0 invisible'
                  )}
                />

                <Panel
                  id="right-main"
                  minSize={MAIN_PANEL_MIN_SIZE}
                  collapsible
                  collapsedSize="0%"
                  className="min-w-0 h-full overflow-hidden"
                >
                  {wikiActive && <WorkspaceWikiArticle actions={wikiActions} />}
                  {rightMainPanelMode === RIGHT_MAIN_PANEL_MODES.CHANGES &&
                    selectedWorkspace?.id && (
                      <ChangesPanelContainer
                        className=""
                        workspaceId={selectedWorkspace.id}
                      />
                    )}
                  {rightMainPanelMode === RIGHT_MAIN_PANEL_MODES.FILES &&
                    selectedWorkspace?.id && (
                      <WorkspaceFilesSurfaceContainer
                        className=""
                        workspaceId={selectedWorkspace.id}
                      />
                    )}
                  {rightMainPanelMode === RIGHT_MAIN_PANEL_MODES.LOGS && (
                    <LogsContentContainer className="" />
                  )}
                  {rightMainPanelMode === RIGHT_MAIN_PANEL_MODES.PREVIEW &&
                    selectedWorkspace?.id && (
                      <PreviewBrowserContainer
                        workspaceId={selectedWorkspace.id}
                        className=""
                      />
                    )}
                </Panel>
              </Group>

              {isRightSidebarVisible && !isCreateMode && (
                <div className="w-[300px] shrink-0 h-full overflow-hidden">
                  <RightSidebar
                    rightMainPanelMode={rightMainPanelMode}
                    selectedWorkspace={selectedWorkspace}
                    repos={repos}
                  />
                </div>
              )}
            </div>
          </MainWorkspaceFilePreviewActionsBridge>
        </WorkspaceFilesSelectionProvider>
      </ChangesViewProvider>
    </ReviewProvider>
  );

  return (
    <WorkspaceWikiProvider
      workspace={selectedWorkspace}
      repos={repos}
      enabled={!isCreateMode && wikiActive}
    >
      <div className="flex flex-1 min-h-0 h-full">
        {isLeftSidebarVisible && (
          <div className="w-[300px] shrink-0 h-full overflow-hidden">
            <WorkspacesSidebarContainer
              onScrollToBottom={handleScrollToBottom}
            />
          </div>
        )}

        <div className="flex-1 min-w-0 h-full">
          {isCreateMode ? (
            <CreateModeProvider
              key={createModeProviderKey}
              initialState={createModeSeed.state}
            >
              {mainContent}
            </CreateModeProvider>
          ) : (
            mainContent
          )}
        </div>
      </div>
    </WorkspaceWikiProvider>
  );
}

function MainWorkspaceFilePreviewActionsBridge({
  children,
  workspaceId,
  setRightMainPanelMode,
}: {
  children: ReactNode;
  workspaceId: string | undefined;
  setRightMainPanelMode: (mode: RightMainPanelMode | null) => void;
}) {
  const { openTarget } = useWorkspaceFilesSelection(workspaceId);

  const handleOpenWorkspaceFilePreview = useCallback(
    (target: WorkspaceFilePreviewTarget) => {
      openTarget(target);
      setRightMainPanelMode(RIGHT_MAIN_PANEL_MODES.FILES);
    },
    [openTarget, setRightMainPanelMode]
  );

  return (
    <WorkspaceFilePreviewActionsProvider
      enabled={Boolean(workspaceId)}
      onOpenWorkspaceFilePreview={handleOpenWorkspaceFilePreview}
    >
      {children}
    </WorkspaceFilePreviewActionsProvider>
  );
}
