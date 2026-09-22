import { FileTreeContainer } from '@/pages/workspaces/FileTreeContainer';
import {
  useDiffs,
  useWorkspaceDiffStore,
} from '@/shared/stores/useWorkspaceDiffStore';
import type { Diff } from 'shared/types';

const diff: Diff = {
  change: 'modified',
  oldPath: 'src/labels.mjs',
  newPath: 'src/labels.mjs',
  oldContent: 'before',
  newContent: 'after',
  additions: 1,
  deletions: 1,
  contentOmitted: false,
  repoId: null,
};

export function DiffTreeHarness() {
  const diffs = useDiffs();
  const update = useWorkspaceDiffStore.setState;
  return (
    <main>
      <button onClick={() => update({ diffError: 'stream unavailable' })}>
        Disconnect
      </button>
      <button
        onClick={() =>
          update({ isDiffInitialized: true, diffError: null, diffs: [] })
        }
      >
        Empty snapshot
      </button>
      <button
        onClick={() =>
          update({ isDiffInitialized: true, diffError: null, diffs: [diff] })
        }
      >
        Changed snapshot
      </button>
      <button
        onClick={() =>
          useWorkspaceDiffStore.getState().clearWorkspaceDiffData()
        }
      >
        Switch workspace
      </button>
      <FileTreeContainer workspaceId="diff-test" diffs={diffs} className="" />
    </main>
  );
}
