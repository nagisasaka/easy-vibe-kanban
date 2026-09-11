import { useEffect, useMemo, useState } from 'react';
import type { Pipeline } from 'shared/types';
import { pipelinesApi } from '@/shared/lib/api';
import {
  inferPipelineSelection,
  updatePipelineBlock,
} from '../model/cardPipeline';

interface PipelineSectionProps {
  description: string;
  disabled?: boolean;
  onDescriptionChange: (description: string) => void;
}

export function PipelineSection({
  description,
  disabled = false,
  onDescriptionChange,
}: PipelineSectionProps) {
  const [pipelines, setPipelines] = useState<Pipeline[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    pipelinesApi
      .list()
      .then((items) => {
        if (!cancelled) setPipelines(items);
      })
      .catch((reason: unknown) => {
        if (!cancelled) {
          setError(
            reason instanceof Error
              ? reason.message
              : 'Unable to load pipelines'
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selection = useMemo(
    () => inferPipelineSelection(description, pipelines),
    [description, pipelines]
  );
  const selected =
    pipelines.find((pipeline) => pipeline.id === selection.pipelineId) ?? null;

  const selectPipeline = (pipelineId: string) => {
    const pipeline =
      pipelines.find((candidate) => candidate.id === pipelineId) ?? null;
    const stages = new Set(
      pipeline?.stages
        .filter((stage) => stage.default_enabled)
        .map((stage) => stage.id) ?? []
    );
    onDescriptionChange(
      updatePipelineBlock(description, pipeline, stages, pipelines)
    );
  };

  const toggleStage = (stageId: string, checked: boolean) => {
    if (!selected) return;
    const stages = new Set(selection.stageIds);
    if (checked) stages.add(stageId);
    else stages.delete(stageId);
    onDescriptionChange(
      updatePipelineBlock(description, selected, stages, pipelines)
    );
  };

  return (
    <section className="border-t px-base py-base space-y-half">
      <div className="flex items-center justify-between gap-base">
        <div>
          <h3 className="text-sm font-medium text-high">Pipeline</h3>
          <p className="text-xs text-low">
            Adds declarative agent guidance to this task.
          </p>
        </div>
        <select
          aria-label="Pipeline"
          value={selection.pipelineId ?? ''}
          disabled={disabled}
          onChange={(event) => selectPipeline(event.target.value)}
          className="rounded border bg-primary px-base py-half text-sm text-high"
        >
          <option value="">None</option>
          {pipelines.map((pipeline) => (
            <option key={pipeline.id} value={pipeline.id}>
              {pipeline.name}
            </option>
          ))}
        </select>
      </div>
      {error && <p className="text-xs text-error">{error}</p>}
      {selected && (
        <div className="space-y-half pt-half">
          {selected.description && (
            <p className="text-xs text-low">{selected.description}</p>
          )}
          {selected.stages.map((stage) => (
            <label
              key={stage.id}
              className="flex items-start gap-half text-sm text-normal"
            >
              <input
                type="checkbox"
                className="mt-[3px]"
                checked={selection.stageIds.has(stage.id)}
                disabled={disabled}
                onChange={(event) =>
                  toggleStage(stage.id, event.target.checked)
                }
              />
              <span>{stage.label}</span>
            </label>
          ))}
        </div>
      )}
    </section>
  );
}
