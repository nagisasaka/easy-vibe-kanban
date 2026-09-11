import { useTranslation } from 'react-i18next';
import type { Pipeline } from 'shared/types';
import {
  inferPipelineSelection,
  updatePipelineBlock,
} from '../model/cardPipeline';
import {
  cardContextPreview,
  hasSharedDirectories,
  splitCardContext,
  toggleSharedDirectories,
  withCardContext,
} from '../model/cardContext';

interface CardContextSectionProps {
  description: string;
  pipelines: Pipeline[];
  error?: string;
  onRetry: () => void;
  disabled?: boolean;
  onDescriptionChange: (description: string) => void;
}

export function CardContextSection({
  description,
  pipelines,
  error,
  onRetry,
  disabled = false,
  onDescriptionChange,
}: CardContextSectionProps) {
  const { t } = useTranslation('common');
  const { context } = splitCardContext(description);
  const selection = inferPipelineSelection(context, pipelines);
  const shared = hasSharedDirectories(context);
  const selected = pipelines.find(
    (pipeline) => pipeline.id === selection.pipelineId
  );
  const labels = [
    selected?.name,
    shared ? t('cardContext.sharedDirectories') : null,
  ].filter(Boolean);
  const update = (next: string) =>
    onDescriptionChange(withCardContext(description, next));

  return (
    <details className="border-t px-base py-base">
      <summary className="cursor-pointer text-sm text-high">
        {t('cardContext.title')}
        <span className="ml-base text-xs text-low">
          {labels.join(' · ') || t('cardContext.none')}
        </span>
        {error && (
          <span className="ml-base text-xs text-error">
            {t('cardContext.loadFailed')}
          </span>
        )}
      </summary>
      <div className="space-y-base pt-base">
        <p className="text-xs text-low">{t('cardContext.description')}</p>
        {error && (
          <p role="alert" className="text-xs text-error">
            {error}
          </p>
        )}
        {error && (
          <button type="button" onClick={onRetry} className="text-sm underline">
            {t('cardContext.retry')}
          </button>
        )}
        {pipelines.map((pipeline) => (
          <label
            key={pipeline.id}
            className="flex items-center gap-half text-sm text-normal"
          >
            <input
              type="checkbox"
              checked={selection.pipelineId === pipeline.id}
              disabled={disabled}
              onChange={(event) =>
                update(
                  updatePipelineBlock(
                    context,
                    event.target.checked ? pipeline : null,
                    pipeline.stages
                      .filter((stage) => stage.default_enabled)
                      .map((stage) => stage.id),
                    pipelines
                  )
                )
              }
            />
            {pipeline.name}
          </label>
        ))}
        <label className="flex items-center gap-half text-sm text-normal">
          <input
            type="checkbox"
            checked={shared}
            disabled={disabled}
            onChange={(event) =>
              update(toggleSharedDirectories(context, event.target.checked))
            }
          />
          {t('cardContext.sharedDirectories')}
        </label>
        {selected &&
          selected.stages.map((stage) => (
            <label
              key={stage.id}
              className="ml-base flex items-center gap-half text-xs text-normal"
            >
              <input
                type="checkbox"
                checked={selection.stageIds.has(stage.id)}
                disabled={disabled}
                onChange={(event) => {
                  const stages = new Set(selection.stageIds);
                  if (event.target.checked) stages.add(stage.id);
                  else stages.delete(stage.id);
                  update(
                    updatePipelineBlock(context, selected, stages, pipelines)
                  );
                }}
              />
              {stage.label}
            </label>
          ))}
        <textarea
          readOnly
          aria-label={t('cardContext.preview')}
          value={cardContextPreview(context)}
          rows={10}
          className="w-full resize-y rounded border bg-primary px-base py-half text-xs text-normal"
        />
      </div>
    </details>
  );
}
