import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  RIGHT_MAIN_PANEL_MODES as MODES,
  useUiPreferencesStore,
} from './useUiPreferencesStore';

const initialState = useUiPreferencesStore.getInitialState();

function viewport(width = 1440) {
  vi.stubGlobal('window', {
    innerWidth: width,
    matchMedia: () => ({ matches: width < 768 }),
  });
}

function panel(workspaceId = 'workspace-a') {
  return useUiPreferencesStore.getState().getWorkspacePanelState(workspaceId);
}

beforeEach(() => {
  useUiPreferencesStore.setState(initialState, true);
  viewport();
});

afterEach(() => vi.unstubAllGlobals());

describe('Wiki reader panel navigation', () => {
  it('opens a focused Wiki with navigation and returns to ordinary chat', () => {
    useUiPreferencesStore.setState({ isRightSidebarVisible: false });
    useUiPreferencesStore.getState().openWiki('workspace-a');
    expect(panel()).toEqual({
      rightMainPanelMode: MODES.WIKI,
      isLeftMainPanelVisible: false,
    });
    expect(useUiPreferencesStore.getState().isRightSidebarVisible).toBe(true);
    expect(useUiPreferencesStore.getState().wikiReturnPanelStates).toEqual({
      'workspace-a': {
        rightMainPanelMode: null,
        isLeftMainPanelVisible: true,
      },
    });
    useUiPreferencesStore.getState().closeWiki('workspace-a');
    expect(panel()).toEqual({
      rightMainPanelMode: null,
      isLeftMainPanelVisible: true,
    });
    expect(useUiPreferencesStore.getState().wikiReturnPanelStates).toEqual({});
  });

  it('preserves the return layout across repeated page opens and split/focus changes', () => {
    const original = {
      rightMainPanelMode: MODES.CHANGES,
      isLeftMainPanelVisible: true,
    };
    useUiPreferencesStore
      .getState()
      .setWorkspacePanelState('workspace-a', original);
    useUiPreferencesStore.getState().openWiki('workspace-a');
    useUiPreferencesStore.getState().toggleLeftMainPanel('workspace-a');
    useUiPreferencesStore.getState().openWiki('workspace-a');
    expect(panel().isLeftMainPanelVisible).toBe(true);
    expect(
      useUiPreferencesStore.getState().wikiReturnPanelStates['workspace-a']
    ).toEqual(original);
    useUiPreferencesStore.getState().toggleLeftMainPanel('workspace-a');
    useUiPreferencesStore.getState().openWiki('workspace-a');
    expect(panel().isLeftMainPanelVisible).toBe(false);
    useUiPreferencesStore.getState().closeWiki('workspace-a');
    expect(panel()).toEqual(original);
  });

  it('restores an intentionally hidden chat instead of overwriting its prior layout', () => {
    const original = {
      rightMainPanelMode: MODES.PREVIEW,
      isLeftMainPanelVisible: false,
    };
    useUiPreferencesStore
      .getState()
      .setWorkspacePanelState('workspace-a', original);
    useUiPreferencesStore.getState().openWiki('workspace-a');
    useUiPreferencesStore
      .getState()
      .setLeftMainPanelVisible(true, 'workspace-a');
    useUiPreferencesStore.getState().closeWiki('workspace-a');
    expect(panel()).toEqual(original);
  });

  it('keeps independent return layouts per workspace', () => {
    useUiPreferencesStore
      .getState()
      .setRightMainPanelMode(MODES.FILES, 'workspace-b');
    useUiPreferencesStore.getState().openWiki('workspace-a');
    useUiPreferencesStore.getState().openWiki('workspace-b');
    useUiPreferencesStore.getState().closeWiki('workspace-a');
    expect(panel('workspace-a').rightMainPanelMode).toBeNull();
    expect(panel('workspace-b').rightMainPanelMode).toBe(MODES.WIKI);
    expect(
      Object.keys(useUiPreferencesStore.getState().wikiReturnPanelStates)
    ).toEqual(['workspace-b']);
    useUiPreferencesStore.getState().closeWiki('workspace-b');
    expect(panel('workspace-b').rightMainPanelMode).toBe(MODES.FILES);
  });

  it('returns to visible chat after reloading with no ephemeral return target', () => {
    useUiPreferencesStore.setState({
      workspacePanelStates: {
        'workspace-a': {
          rightMainPanelMode: MODES.WIKI,
          isLeftMainPanelVisible: false,
        },
      },
    });
    useUiPreferencesStore.getState().openWiki('workspace-a');
    expect(useUiPreferencesStore.getState().wikiReturnPanelStates).toEqual({});
    useUiPreferencesStore.getState().closeWiki('workspace-a');
    expect(panel()).toEqual({
      rightMainPanelMode: null,
      isLeftMainPanelVisible: true,
    });
  });

  it.each(['setRightMainPanelMode', 'toggleRightMainPanelMode'] as const)(
    '%s delegates Wiki navigation to the same focus/return contract',
    (action) => {
      useUiPreferencesStore
        .getState()
        .setRightMainPanelMode(MODES.LOGS, 'workspace-a');
      useUiPreferencesStore.getState()[action](MODES.WIKI, 'workspace-a');
      expect(panel().isLeftMainPanelVisible).toBe(false);
      if (action === 'toggleRightMainPanelMode') {
        useUiPreferencesStore.getState()[action](MODES.WIKI, 'workspace-a');
      } else {
        useUiPreferencesStore.getState()[action](null, 'workspace-a');
      }
      expect(panel()).toEqual({
        rightMainPanelMode: MODES.LOGS,
        isLeftMainPanelVisible: true,
      });
    }
  );

  it.each(['setRightMainPanelMode', 'toggleRightMainPanelMode'] as const)(
    '%s restores chat visibility when navigating directly from Wiki to another panel',
    (action) => {
      useUiPreferencesStore.getState().openWiki('workspace-a');
      useUiPreferencesStore.getState()[action](MODES.FILES, 'workspace-a');
      expect(panel()).toEqual({
        rightMainPanelMode: MODES.FILES,
        isLeftMainPanelVisible: true,
      });
      expect(useUiPreferencesStore.getState().wikiReturnPanelStates).toEqual(
        {}
      );
      // A stale return action must not reopen the earlier Wiki/layout.
      useUiPreferencesStore.getState().closeWiki('workspace-a');
      expect(panel().rightMainPanelMode).toBe(MODES.FILES);
    }
  );

  it('does not carry focused Wiki state into the next visit after selecting another mode', () => {
    useUiPreferencesStore.getState().openWiki('workspace-a');
    useUiPreferencesStore
      .getState()
      .setRightMainPanelMode(MODES.PREVIEW, 'workspace-a');
    useUiPreferencesStore.getState().openWiki('workspace-a');
    useUiPreferencesStore.getState().closeWiki('workspace-a');
    expect(panel()).toEqual({
      rightMainPanelMode: MODES.PREVIEW,
      isLeftMainPanelVisible: true,
    });
  });

  it('has deterministic direct navigation away from a reloaded focused Wiki', () => {
    useUiPreferencesStore.getState().setWorkspacePanelState('workspace-a', {
      rightMainPanelMode: MODES.WIKI,
      isLeftMainPanelVisible: false,
    });
    useUiPreferencesStore
      .getState()
      .setRightMainPanelMode(MODES.FILES, 'workspace-a');
    expect(panel().isLeftMainPanelVisible).toBe(true);
  });

  it('activates the mobile Wiki pane and returns to the mobile Chat tab', () => {
    viewport(390);
    useUiPreferencesStore.getState().openWiki('workspace-a');
    expect(useUiPreferencesStore.getState().mobileActiveTab).toBe('wiki');
    useUiPreferencesStore.getState().setMobileActiveTab('chat');
    useUiPreferencesStore.getState().openWiki('workspace-a');
    expect(useUiPreferencesStore.getState().mobileActiveTab).toBe('wiki');
    useUiPreferencesStore.getState().closeWiki('workspace-a');
    expect(useUiPreferencesStore.getState().mobileActiveTab).toBe('chat');
  });

  it('does not mutate mobile tab preferences when desktop opens the reader', () => {
    useUiPreferencesStore.setState({ mobileActiveTab: 'chat' });
    useUiPreferencesStore.getState().openWiki('workspace-a');
    expect(useUiPreferencesStore.getState().mobileActiveTab).toBe('chat');
  });

  it('is a safe no-op without a selected workspace', () => {
    const before = useUiPreferencesStore.getState();
    before.openWiki();
    before.closeWiki();
    before.toggleRightMainPanelMode(MODES.WIKI);
    expect(useUiPreferencesStore.getState()).toBe(before);
  });
});
