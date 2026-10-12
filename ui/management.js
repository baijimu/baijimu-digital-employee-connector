const notice = document.querySelector('#notice');
const employeeForm = document.querySelector('#employee-form');
const executionForm = document.querySelector('#execution-form');
const authorizationForm = document.querySelector('#authorization-form');
let busy = false;
let registered = false;
let workspaceChoices = [];
let workspacesLoaded = false;

function report(message, error = false) {
  notice.textContent = message;
  notice.dataset.error = String(error);
}

function lock(value) {
  busy = value;
  document.querySelectorAll('button').forEach(button => { button.disabled = value; });
  document.querySelector('#employee-fields').disabled = value;
  document.querySelector('#execution-fields').disabled = value || !registered;
  document.querySelector('#authorization-fields').disabled = value || !registered;
}

async function operation(name, payload, apply) {
  if (busy) return;
  lock(true);
  report('正在处理…');
  try {
    const result = await window.baijimuLocalApp.invoke(name, payload);
    if (apply) await apply(result);
    report('操作已完成。');
  } catch (error) {
    report(error instanceof Error ? error.message : String(error), true);
  } finally {
    lock(false);
  }
}

function render(state) {
  const employee = state.employee;
  registered = employee !== null;
  document.querySelector('#registration-status').textContent = !registered
    ? '未注册' : employee.enabled ? '已注册 · 已启用' : '已注册 · 已停用';
  document.querySelector('#save-employee').textContent = registered ? '保存员工资料' : '注册员工';
  employeeForm.elements.name.value = employee?.name ?? '';
  employeeForm.elements.description.value = employee?.description ?? '';
  employeeForm.elements.enabled.checked = employee?.enabled ?? true;
  executionForm.elements.workingDirectory.value = employee?.workingDirectory ?? '';
  const workspaces = document.querySelector('#workspaces');
  workspaces.replaceChildren();
  const granted = new Set(employee?.authorizedWorkspaceIds ?? []);
  const choices = new Map(workspaceChoices.map(workspace => [workspace.workspaceId, workspace.name]));
  for (const id of granted) {
    if (!choices.has(id)) choices.set(id, `工作区 ${id}（${workspacesLoaded ? '当前账户不可访问，可撤销' : '已授权，尚未核对当前访问权'}）`);
  }
  for (const [id, name] of choices) {
    const label = document.createElement('label');
    label.className = 'checkbox';
    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    checkbox.name = 'workspaceId';
    checkbox.value = String(id);
    checkbox.checked = granted.has(id);
    const text = document.createElement('span');
    text.textContent = `${name} · ${id}`;
    label.append(checkbox, text);
    workspaces.append(label);
  }
  if (choices.size === 0) workspaces.textContent = workspacesLoaded
    ? '当前本机账号没有可授权的工作区。请先在客户端完成平台授权。'
    : '请读取当前本机账号可访问的工作区。';
}

employeeForm.addEventListener('submit', event => {
  event.preventDefault();
  void operation(registered ? 'employeeUpdate' : 'employeeRegister', {
    name: employeeForm.elements.name.value.trim(),
    description: employeeForm.elements.description.value.trim(),
    enabled: employeeForm.elements.enabled.checked,
  }, render);
});

executionForm.addEventListener('submit', event => {
  event.preventDefault();
  void operation('employeeConfigure', {
    workingDirectory: executionForm.elements.workingDirectory.value.trim(),
  }, render);
});

authorizationForm.addEventListener('submit', event => {
  event.preventDefault();
  const authorizedWorkspaceIds = [...authorizationForm.querySelectorAll('input:checked')]
    .map(input => Number(input.value));
  if (authorizedWorkspaceIds.some(id => !Number.isSafeInteger(id) || id <= 0)) {
    report('工作区身份无法安全表示，请核对客户端和平台目录。', true);
    return;
  }
  void operation('employeeAuthorize', { authorizedWorkspaceIds }, render);
});

function account(result) {
  document.querySelector('#account-status').textContent = result.account
    ? `${result.account.email ?? result.account.type}${result.account.planType ? ` · ${result.account.planType}` : ''}`
    : '尚未登录';
}

function login(result) {
  const url = new URL(result.authUrl);
  if (url.protocol !== 'https:') throw new Error('登录服务返回了不安全的地址');
  document.querySelector('#login-url').value = url.toString();
  document.querySelector('#login-result').hidden = false;
}

document.querySelectorAll('[data-operation]').forEach(button => {
  button.addEventListener('click', () => {
    const name = button.dataset.operation;
    const apply = name === 'accountRead' ? account : name === 'accountLogin' ? login
      : name === 'employeeWorkspaces' ? result => {
        workspaceChoices = result.workspaces;
        workspacesLoaded = true;
        return window.baijimuLocalApp.invoke('employeeState').then(render);
      } : () => {
        document.querySelector('#account-status').textContent = '已退出';
        document.querySelector('#login-result').hidden = true;
      };
    void operation(name, undefined, apply);
  });
});

if (window.baijimuLocalApp) {
  void operation('employeeState', undefined, render);
} else {
  report('管理界面需要通过本机百积木客户端打开。', true);
}
