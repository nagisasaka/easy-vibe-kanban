import { expect, test, type Page } from '@playwright/test';

function document(id: string) {
  return {
    id,
    source: 'project',
    project_id: 'project',
    name: `Workflow ${id}`,
    description: 'Original description',
    revision: 1,
    created_at: '',
    updated_at: '',
    graph_json: JSON.stringify({
      version: 2,
      nodes: [
        {
          id: 'start',
          type: 'start',
          data: { display_name: 'Start' },
          position: { x: 50, y: 100 },
        },
        {
          id: 'agent',
          type: 'agent',
          data: {
            display_name: 'Agent',
            prompt_template: 'Original',
            include_workflow_context: true,
            executor_config: { executor: 'CODEX', reasoning_id: 'max' },
          },
          position: { x: 380, y: 100 },
        },
        {
          id: 'end',
          type: 'end',
          data: { display_name: 'End' },
          position: { x: 800, y: 100 },
        },
      ],
      edges: [
        {
          id: 'start-agent',
          source: 'start',
          target: 'agent',
          type: 'default',
        },
        { id: 'agent-end', source: 'agent', target: 'end', type: 'default' },
      ],
    }),
  };
}

async function setup(page: Page) {
  await page.route('**/api/agents/preset-options?**', (route) =>
    route.fulfill({ json: { success: true, data: { executor: 'CODEX' } } })
  );
  const control = {
    documents: { a: document('a'), b: document('b') },
    fail: false,
    failRead: false,
    hold: null as (() => Promise<void>) | null,
    writes: [] as any[],
  };
  await page.route('**/api/local/v1/workflows/*', async (route) => {
    const id = route.request().url().split('/').at(-1) as 'a' | 'b';
    if (route.request().method() === 'GET')
      return route.fulfill(
        control.failRead
          ? { status: 503, json: { message: 'Unavailable' } }
          : { json: control.documents[id] }
      );
    const body = route.request().postDataJSON();
    control.writes.push(body);
    if (control.hold) await control.hold();
    if (control.fail)
      return route.fulfill({ status: 500, json: { message: 'Save failed' } });
    if (body.expected_revision !== control.documents[id].revision)
      return route.fulfill({
        status: 409,
        json: { message: 'Revision conflict' },
      });
    const graph = JSON.parse(body.graph_json);
    graph.nodes.find((node: any) => node.type === 'agent').data.session_id =
      'assigned-session';
    control.documents[id] = {
      ...control.documents[id],
      ...body,
      graph_json: JSON.stringify(graph),
      revision: body.expected_revision + 1,
    };
    return route.fulfill({ json: { data: control.documents[id], txid: 1 } });
  });
  await page.goto('/?mode=workflow-draft');
  await expect(page.getByLabel('Draft name')).toHaveValue('Workflow a');
  return control;
}
const state = (page: Page) => page.getByTestId('draft-state');
const readState = async (page: Page) =>
  JSON.parse((await state(page).textContent())!);

test('late save ACK preserves newer edits; undo retains server Session and all graph fields', async ({
  page,
}) => {
  const control = await setup(page);
  let release!: () => void;
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  control.hold = () => pending;
  await page.getByRole('button', { name: 'Edit graph contract' }).click();
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect.poll(() => control.writes.length).toBe(1);
  await page.getByLabel('Draft description').fill('Newer draft');
  release();
  await expect(state(page)).toContainText('"saving":false');
  expect((await readState(page)).dirty).toBe(true);
  await expect(page.getByLabel('Draft description')).toHaveValue('Newer draft');
  await page.getByRole('button', { name: /^Undo workflow/ }).click();
  expect((await readState(page)).dirty).toBe(false);
  let node = (await readState(page)).value.graph.nodes[1];
  expect(node.data).toMatchObject({
    session_id: 'assigned-session',
    selected_skills: [{ name: 'retained', path: '/fixture/skill/SKILL.md' }],
    include_workflow_context: false,
    executor_config: { reasoning_id: 'max' },
  });
  await page.getByRole('button', { name: /^Undo workflow/ }).click();
  node = (await readState(page)).value.graph.nodes[1];
  expect(node.data.session_id).toBe('assigned-session');
  expect(node.data.prompt_template).toBe('Original');
  await page.keyboard.press('Control+Shift+Z');
  expect(
    (await readState(page)).value.graph.nodes[1].data.prompt_template
  ).toBe('Changed prompt');
});

test('refetch/409 never replaces dirty draft or original revision, including reload', async ({
  page,
}) => {
  const control = await setup(page);
  await page.getByLabel('Draft name').fill('My draft');
  control.documents.a = {
    ...control.documents.a,
    name: 'Other editor',
    revision: 2,
  };
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Revision conflict');
  await expect(state(page)).toContainText('"conflict":true');
  await expect(page.getByLabel('Draft name')).toHaveValue('My draft');
  expect(control.writes[0].expected_revision).toBe(1);
  page.once('dialog', (dialog) => dialog.accept());
  await page.reload();
  await expect(page.getByLabel('Draft name')).toHaveValue('My draft');
  await expect(state(page)).toContainText('"revision":1');
  await page
    .getByRole('button', { name: 'Discard draft', exact: true })
    .click();
  await expect(page.getByLabel('Draft name')).toHaveValue('Other editor');
  expect((await readState(page)).dirty).toBe(false);
});

test('failed save-and-leave retains draft, continue restores focus, discard leaves', async ({
  page,
}) => {
  const control = await setup(page);
  control.fail = true;
  await page.getByLabel('Draft name').fill('Retained');
  await page.getByRole('link', { name: 'Leave editor' }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page
    .getByRole('button', { name: 'Save and leave', exact: true })
    .click();
  await expect(page.getByRole('alert')).toContainText('Save failed');
  await expect(page.getByLabel('Draft name')).toHaveValue('Retained');
  await page
    .getByRole('button', { name: 'Continue editing', exact: true })
    .click();
  await expect(page.getByRole('link', { name: 'Leave editor' })).toBeFocused();
  await page.getByRole('link', { name: 'Leave editor' }).click();
  await page
    .getByRole('button', { name: 'Discard and leave', exact: true })
    .click();
  await expect(page.getByText('Other page')).toBeVisible();
});

test('executor selection follows graph Undo/Redo; native text undo is not hijacked', async ({
  page,
}) => {
  await setup(page);
  await expect(page.getByTestId('controlled-executor')).toContainText(
    '"reasoning_id":"max"'
  );
  await page.getByRole('button', { name: 'Set effort ultra' }).click();
  await expect(page.getByTestId('controlled-executor')).toContainText(
    '"reasoning_id":"ultra"'
  );
  await page.getByRole('button', { name: /^Undo workflow/ }).click();
  await expect(page.getByTestId('controlled-executor')).toContainText(
    '"reasoning_id":"max"'
  );
  await page.getByRole('button', { name: /^Redo workflow/ }).click();
  await expect(page.getByTestId('controlled-executor')).toContainText(
    '"reasoning_id":"ultra"'
  );
  const description = page.getByLabel('Draft description');
  await description.focus();
  await description.press('End');
  // Native undo transaction grouping is browser-dependent; use one keystroke.
  await description.press('!');
  await description.press('Control+Z');
  await expect(description).toHaveValue('Original description');
  await expect(
    page.getByRole('button', { name: /^Redo workflow/ })
  ).toBeDisabled();
  await expect(page.getByTestId('controlled-executor')).toContainText(
    '"reasoning_id":"ultra"'
  );
});

test('canvas drag participates in bounded history and save ACK clears reload draft', async ({
  page,
}) => {
  await setup(page);
  const node = page
    .locator('.react-flow__node')
    .filter({ has: page.getByTestId('workflow-node-agent') });
  await expect(node).toBeVisible();
  const before = (await readState(page)).value.graph.nodes[1].position;
  const box = (await node.boundingBox())!;
  const x = box.x + box.width / 2,
    y = box.y + box.height - 12;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 90, y + 65, { steps: 8 });
  await page.mouse.up();
  await expect
    .poll(async () => (await readState(page)).value.graph.nodes[1].position)
    .not.toEqual(before);
  while (await page.getByRole('button', { name: /^Undo workflow/ }).isEnabled())
    await page.getByRole('button', { name: /^Undo workflow/ }).click();
  expect((await readState(page)).value.graph.nodes[1].position).toEqual(before);
  await page.getByLabel('Draft name').fill('Persisted before leave');
  await page.getByRole('link', { name: 'Leave editor' }).click();
  await page
    .getByRole('button', { name: 'Save and leave', exact: true })
    .click();
  await expect(page.getByText('Other page')).toBeVisible();
  expect(
    await page.evaluate(() =>
      Object.keys(sessionStorage).filter((key) =>
        key.startsWith('vibe.workflowEditorDraft.')
      )
    )
  ).toEqual([]);
  await page.reload();
  await expect(page.getByLabel('Draft name')).toHaveValue(
    'Persisted before leave'
  );
  expect((await readState(page)).dirty).toBe(false);
});

test('successful save-and-leave and target change isolate pending ACKs', async ({
  page,
}) => {
  const control = await setup(page);
  let release!: () => void;
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  control.hold = () => pending;
  await page.getByLabel('Draft name').fill('A pending');
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect.poll(() => control.writes.length).toBe(1);
  await page.getByRole('button', { name: 'Switch target' }).click();
  await expect(page.getByLabel('Draft name')).toHaveValue('Workflow b');
  release();
  await expect.poll(() => control.documents.a.name).toBe('A pending');
  expect((await readState(page)).revision).toBe(1);
  await expect(
    page.getByRole('button', { name: /^Undo workflow/ })
  ).toBeDisabled();
  control.hold = null;
  await page.getByLabel('Draft name').fill('B change');
  await page.getByRole('link', { name: 'Leave editor' }).click();
  await page
    .getByRole('button', { name: 'Save and leave', exact: true })
    .click();
  await expect(page.getByText('Other page')).toBeVisible();
});

test('cached fetch errors and system templates remain read-only', async ({
  page,
}) => {
  const control = await setup(page);
  await page.getByLabel('Draft name').fill('Preserved cached draft');
  control.failRead = true;
  await page.getByRole('button', { name: 'Refetch baseline' }).click();
  await expect(page.getByLabel('Draft name')).toBeDisabled();
  await expect(page.getByLabel('Draft name')).toHaveValue(
    'Preserved cached draft'
  );
  await expect(
    page.getByRole('button', { name: /^Undo workflow/ })
  ).toBeDisabled();
  control.failRead = false;
  control.documents.b.source = 'system';
  await page.getByRole('button', { name: 'Switch target' }).click();
  await expect(page.getByLabel('Draft name')).toHaveValue('Workflow b');
  await expect(page.getByLabel('Draft name')).toBeDisabled();
  await expect(
    page.getByRole('button', { name: 'Edit graph contract' })
  ).toBeDisabled();
});
