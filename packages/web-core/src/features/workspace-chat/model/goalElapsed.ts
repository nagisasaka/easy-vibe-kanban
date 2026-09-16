/** Interpolate provider-reported time only while this Goal has an active run. */
export function watchGoalElapsed(
  reportedSeconds: number,
  goalActive: boolean,
  runActive: boolean,
  update: (seconds: number) => void
) {
  update(reportedSeconds);
  if (!goalActive || !runActive) return;
  const started = Date.now();
  const timer = setInterval(() => {
    update(reportedSeconds + Math.floor((Date.now() - started) / 1000));
  }, 1000);
  return () => clearInterval(timer);
}
