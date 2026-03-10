'use strict';

const App = (() => {
  const state = {
    account: null,
    tenants: []
  };

  const GEMINI_MODEL_OPTIONS = [
    {
      value: 'gemini-2.5-flash',
      label: 'Gemini 2.5 Flash — balanced default',
      help: 'Best default for everyday supportive chat. Fast, capable, and usually gentler on quota than a heavier model.'
    },
    {
      value: 'gemini-2.5-pro',
      label: 'Gemini 2.5 Pro — strongest but heaviest',
      help: 'Usually the most capable preset here, but it will likely consume quota faster and may respond more slowly.'
    },
    {
      value: 'gemini-2.0-flash',
      label: 'Gemini 2.0 Flash — lighter/quota-friendly',
      help: 'A lighter preset for quick check-ins and lower quota pressure when you want something cheaper to run.'
    }
  ];

  function $(sel, ctx = document) {
    return ctx.querySelector(sel);
  }

  function $$(sel, ctx = document) {
    return [...ctx.querySelectorAll(sel)];
  }

  function on(el, evt, fn, opts) {
    if (el) el.addEventListener(evt, fn, opts);
  }

  let toastTimer = null;
  function toast(message, duration = 3200) {
    let el = $('#toast');
    if (!el) {
      el = document.createElement('div');
      el.id = 'toast';
      el.className = 'toast';
      el.setAttribute('role', 'status');
      el.setAttribute('aria-live', 'polite');
      document.body.appendChild(el);
    }
    el.textContent = message;
    el.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.classList.remove('show'), duration);
  }

  async function api(path, options = {}) {
    const headers = new Headers(options.headers || {});
    if (options.body && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json');
    }

    const response = await fetch(path, {
      ...options,
      headers,
      credentials: 'same-origin'
    });

    const contentType = response.headers.get('content-type') || '';
    const body = contentType.includes('application/json')
      ? await response.json()
      : await response.text();

    if (!response.ok) {
      const message = typeof body === 'object' && body?.error
        ? body.error
        : `Request failed with ${response.status}`;
      throw new Error(message);
    }

    return body;
  }

  function queryTenantId() {
    return optionalValue(new URLSearchParams(window.location.search).get('tenant'));
  }

  function currentTenantId() {
    return queryTenantId() || optionalValue(window.localStorage.getItem('wb_tenant_id'));
  }

  function rememberTenantId(value) {
    const tenantId = optionalValue(value);
    if (tenantId) {
      window.localStorage.setItem('wb_tenant_id', tenantId);
    }
  }

  async function loadTenants() {
    if (state.tenants.length > 0) {
      return state.tenants;
    }

    try {
      state.tenants = await api('/tenants');
    } catch (error) {
      state.tenants = [];
    }
    return state.tenants;
  }

  async function initTenantSelectors() {
    const selects = $$('[data-tenant-select]');
    if (!selects.length) return;

    const tenants = await loadTenants();
    if (!tenants.length) return;

    const selectedTenantId = currentTenantId() || tenants[0].id;
    selects.forEach((select) => {
      select.innerHTML = '';
      tenants.forEach((tenant) => {
        const option = document.createElement('option');
        option.value = tenant.id;
        option.textContent = tenant.display_name;
        option.selected = tenant.id === selectedTenantId;
        select.appendChild(option);
      });
      rememberTenantId(select.value);
      on(select, 'change', () => rememberTenantId(select.value));
    });
  }

  function syncTenantLinks() {
    const tenantId = currentTenantId();
    if (!tenantId) return;

    $$('[data-tenant-link]').forEach((link) => {
      const url = new URL(link.getAttribute('href'), window.location.origin);
      url.searchParams.set('tenant', tenantId);
      const hash = url.hash;
      const search = url.searchParams.toString();
      link.setAttribute('href', `${url.pathname}${search ? `?${search}` : ''}${hash}`);
    });
  }

  async function loadSession() {
    try {
      const data = await api('/api/auth/me');
      state.account = data.account;
      rememberTenantId(data.account.tenant_id);
      applyAccountChrome(state.account);
      return data.account;
    } catch (error) {
      state.account = null;
      return null;
    }
  }

  function applyAccountChrome(account) {
    if (!account) return;
    const initial = (account.profile.user_name || account.email || 'Y').trim().charAt(0).toUpperCase();
    $$('#avatar-btn span').forEach((el) => {
      el.textContent = initial;
    });
  }

  function initTabs(container) {
    const tabButtons = $$('.tab', container);
    const tabPanels = $$('.tab-content', container);

    tabButtons.forEach((btn) => {
      on(btn, 'click', () => {
        const target = btn.dataset.tab;
        tabButtons.forEach((b) => b.classList.remove('active'));
        tabPanels.forEach((p) => p.classList.remove('active'));
        btn.classList.add('active');
        const panel = $(`#${target}`, container);
        if (panel) panel.classList.add('active');
      });
    });
  }

  function initStepper() {
    const steps = $$('.step');
    const dots = $$('.stepper-dot');
    if (!steps.length) return null;

    let current = 0;

    function goTo(idx) {
      if (idx < 0 || idx >= steps.length) return;
      steps.forEach((s) => s.classList.remove('active'));
      dots.forEach((d) => d.classList.remove('active', 'done'));

      steps[idx].classList.add('active');
      dots.forEach((dot, i) => {
        if (i < idx) dot.classList.add('done');
        if (i === idx) dot.classList.add('active');
      });
      current = idx;
      steps[idx].scrollIntoView({ behavior: 'smooth', block: 'start' });
    }

    $$('[data-step-next]').forEach((btn) => on(btn, 'click', () => goTo(current + 1)));
    $$('[data-step-prev]').forEach((btn) => on(btn, 'click', () => goTo(current - 1)));

    goTo(0);
    return { goTo };
  }

  function initModals() {
    $$('[data-modal-open]').forEach((btn) => {
      on(btn, 'click', () => {
        $(`#${btn.dataset.modalOpen}`)?.classList.add('open');
      });
    });

    $$('[data-modal-close]').forEach((btn) => {
      on(btn, 'click', () => btn.closest('.modal-backdrop')?.classList.remove('open'));
    });

    $$('.modal-backdrop').forEach((backdrop) => {
      on(backdrop, 'click', (event) => {
        if (event.target === backdrop) backdrop.classList.remove('open');
      });
    });

    on(document, 'keydown', (event) => {
      if (event.key === 'Escape') {
        $$('.modal-backdrop.open').forEach((modal) => modal.classList.remove('open'));
      }
    });
  }

  function browserTimezone() {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  }

  function supportedTimezones() {
    if (typeof Intl.supportedValuesOf === 'function') {
      return Intl.supportedValuesOf('timeZone');
    }

    const fallback = ['UTC'];
    const browser = browserTimezone();
    if (!fallback.includes(browser)) {
      fallback.push(browser);
    }
    return fallback;
  }

  function formatTimezoneLabel(timezone) {
    return timezone.replaceAll('_', ' ');
  }

  function populateTimezoneSelects() {
    const timezones = supportedTimezones();
    $$('[data-timezone-select]').forEach((select) => {
      if (select.dataset.populated === 'true') return;

      const selected = select.dataset.selectedTimezone || select.value || browserTimezone();
      select.innerHTML = '';

      timezones.forEach((timezone) => {
        const option = document.createElement('option');
        option.value = timezone;
        option.textContent = formatTimezoneLabel(timezone);
        if (timezone === selected) {
          option.selected = true;
        }
        select.appendChild(option);
      });

      if (![...select.options].some((option) => option.value === selected)) {
        const option = document.createElement('option');
        option.value = selected;
        option.textContent = formatTimezoneLabel(selected);
        option.selected = true;
        select.prepend(option);
      }

      select.dataset.populated = 'true';
    });
  }

  function populateGeminiModelSelects() {
    $$('[data-gemini-model-select]').forEach((select) => {
      if (select.dataset.populated === 'true') return;

      const selected = select.dataset.selectedModel || select.value || GEMINI_MODEL_OPTIONS[0].value;
      select.innerHTML = '';

      GEMINI_MODEL_OPTIONS.forEach((model) => {
        const option = document.createElement('option');
        option.value = model.value;
        option.textContent = model.label;
        option.selected = model.value === selected;
        select.appendChild(option);
      });

      if (![...select.options].some((option) => option.value === selected) && selected) {
        const customOption = document.createElement('option');
        customOption.value = selected;
        customOption.textContent = `${selected} — saved custom model`;
        customOption.selected = true;
        select.prepend(customOption);
      }

      select.dataset.populated = 'true';
      updateGeminiModelHelp(select);
      on(select, 'change', () => updateGeminiModelHelp(select));
    });
  }

  function updateGeminiModelHelp(select) {
    const container = select.closest('.form-group');
    const helper = $('[data-gemini-model-help]', container);
    if (!helper) return;

    const match = GEMINI_MODEL_OPTIONS.find((model) => model.value === select.value);
    helper.textContent = match
      ? match.help
      : 'Saved custom model name. If Google retires or renames it later, switch back to one of the presets above.';
  }

  function checkinTimeToClock(label) {
    switch (label) {
      case 'morning': return '09:00';
      case 'afternoon': return '14:00';
      case 'evening': return '19:00';
      case 'anytime': return '12:00';
      default: return '19:00';
    }
  }

  function localTimeToLabel(value) {
    if (value >= '05:00' && value < '12:00') return 'morning';
    if (value >= '12:00' && value < '17:00') return 'afternoon';
    if (value >= '17:00' && value <= '23:00') return 'evening';
    return 'anytime';
  }

  function frequencyToDays(value) {
    switch (value) {
      case 'daily': return [1, 2, 3, 4, 5, 6, 7];
      case 'few_times': return [1, 3, 5];
      case 'weekly': return [1];
      default: return [];
    }
  }

  function buildProfilePayload(form) {
    const data = Object.fromEntries(new FormData(form).entries());
    const frequency = data.checkin_frequency || 'never';
    const checkinsEnabled = frequency !== 'never';
    const personalInferenceEnabled = Boolean($('[name="personal_inference_enabled"]', form)?.checked);

    return {
      companion_name: data.bot_name || 'Hope',
      user_name: optionalValue(data.user_name),
      pronouns: optionalValue(data.pronouns),
      user_context: null,
      boundaries: optionalValue(data.boundaries),
      support_goals: optionalValue(data.goals),
      preferred_style: optionalValue(data.checkin_style),
      companion_tone: optionalValue(data.companion_tone),
      checkin_frequency: optionalValue(frequency),
      checkin_style: optionalValue(data.checkin_style),
      telegram_bot_token: optionalValue(data.telegram_token),
      telegram_bot_username: optionalValue(data.telegram_username),
      personal_inference_enabled: personalInferenceEnabled,
      personal_inference_model: personalInferenceEnabled
        ? (optionalValue(data.personal_inference_model) || GEMINI_MODEL_OPTIONS[0].value)
        : null,
      personal_inference_api_key: personalInferenceEnabled
        ? optionalValue(data.personal_inference_api_key)
        : null,
      onboarding_complete: true,
      checkins_enabled: checkinsEnabled,
      timezone: optionalValue(data.timezone) || browserTimezone(),
      checkin_local_time: checkinTimeToClock(data.checkin_time || 'evening'),
      checkin_days: frequencyToDays(frequency),
      quiet_hours: ['22:00-07:00']
    };
  }

  function optionalValue(value) {
    if (value == null) return null;
    const trimmed = String(value).trim();
    return trimmed ? trimmed : null;
  }

  function fillProfileForm(form, profile) {
    setValue(form, 'user_name', profile.user_name);
    setValue(form, 'pronouns', profile.pronouns);
    setValue(form, 'bot_name', profile.companion_name);
    setValue(form, 'companion_tone', profile.companion_tone || 'warm');
    setValue(form, 'goals', profile.support_goals);
    setValue(form, 'boundaries', profile.boundaries);
    setValue(form, 'checkin_frequency', profile.checkin_frequency || (profile.checkins_enabled ? 'daily' : 'never'));
    setValue(form, 'checkin_time', localTimeToLabel(profile.checkin_local_time || '19:00'));
    setValue(form, 'checkin_style', profile.checkin_style || 'mixed');
    setValue(form, 'timezone', profile.timezone || browserTimezone());
    setValue(form, 'telegram_token', profile.telegram_bot_token);
    setValue(form, 'telegram_username', profile.telegram_bot_username);
    setValue(form, 'personal_inference_model', profile.personal_inference_model || GEMINI_MODEL_OPTIONS[0].value);
    const inferenceToggle = $('[name="personal_inference_enabled"]', form);
    if (inferenceToggle) {
      inferenceToggle.checked = Boolean(profile.personal_inference_enabled);
    }
    const keyField = $('[name="personal_inference_api_key"]', form);
    if (keyField) {
      keyField.value = '';
    }
    updatePersonalInferenceStatus(form, profile);
  }

  function setValue(form, name, value) {
    const input = $(`[name="${name}"]`, form);
    if (!input || value == null) return;
    input.value = value;
  }

  function updatePersonalInferenceStatus(form, profile = {}) {
    const enabled = Boolean($('[name="personal_inference_enabled"]', form)?.checked);
    const configured = Boolean(profile.personal_inference_api_key_configured);
    const modelInput = $('[name="personal_inference_model"]', form);
    const keyInput = $('[name="personal_inference_api_key"]', form);
    const status = $('[data-personal-key-status]', form);

    [modelInput, keyInput].forEach((input) => {
      if (input) input.disabled = !enabled;
    });

    if (!status) return;
    if (!enabled) {
      status.textContent = 'Leave this off only if this companion already has a shared Gemini key configured by the operator.';
      return;
    }

    status.textContent = configured
      ? 'A Gemini key is already stored securely. Leave the key field blank to keep it, or paste a new one to rotate it.'
      : 'If you add a Gemini key, it will be encrypted before it is stored.';
  }

  function initPersonalInferenceControls(form, profile = {}) {
    const toggle = $('[name="personal_inference_enabled"]', form);
    if (!toggle) return;
    updateGeminiModelHelp($('[name="personal_inference_model"]', form));
    updatePersonalInferenceStatus(form, profile);
    on(toggle, 'change', () => updatePersonalInferenceStatus(form, profile));
  }

  async function requireSession(options = {}) {
    const account = await loadSession();
    if (!account) {
      if (!options.silent) window.location.href = '/login.html';
      return null;
    }
    return account;
  }

  async function initLoginForm() {
    const loginForm = $('#login-form');
    const signupForm = $('#signup-form');
    if (!loginForm && !signupForm) return;

    const existing = await loadSession();
    if (existing) {
      window.location.href = existing.profile.onboarding_complete ? '/chat.html' : '/onboarding.html';
      return;
    }

    if (window.location.hash === '#signup') {
      $('#tab-btn-signup')?.click();
    }

    if (loginForm) {
      on(loginForm, 'submit', async (event) => {
        event.preventDefault();
        try {
          const form = new FormData(loginForm);
          const response = await api('/api/auth/login', {
            method: 'POST',
            body: JSON.stringify({
              email: form.get('email'),
              password: form.get('password'),
              tenant_id: optionalValue(form.get('tenant_id')) || currentTenantId()
            })
          });
          state.account = response.account;
          rememberTenantId(response.account.tenant_id);
          toast('Signed in');
          window.location.href = response.account.profile.onboarding_complete ? '/chat.html' : '/onboarding.html';
        } catch (error) {
          toast(error.message);
        }
      });
    }

    if (signupForm) {
      on(signupForm, 'submit', async (event) => {
        event.preventDefault();
        try {
          const form = new FormData(signupForm);
          const response = await api('/api/auth/signup', {
            method: 'POST',
            body: JSON.stringify({
              email: form.get('email'),
              password: form.get('password'),
              tenant_id: optionalValue(form.get('tenant_id')) || currentTenantId()
            })
          });
          state.account = response.account;
          rememberTenantId(response.account.tenant_id);
          toast('Account created');
          window.location.href = '/onboarding.html';
        } catch (error) {
          toast(error.message);
        }
      });
    }
  }

  async function initOnboardingForm() {
    const form = $('#onboarding-form');
    if (!form) return;

    const account = await requireSession();
    if (!account) return;
    fillProfileForm(form, account.profile);
    initPersonalInferenceControls(form, account.profile);
    initStepper();

    on(form, 'submit', async (event) => {
      event.preventDefault();
      try {
        const profile = await api('/api/me/profile', {
          method: 'PUT',
          body: JSON.stringify(buildProfilePayload(form))
        });
        state.account.profile = profile.profile;
        rememberTenantId(state.account.tenant_id);
        toast('You’re all set');
        window.location.href = '/chat.html';
      } catch (error) {
        toast(error.message);
      }
    });
  }

  async function initSettingsForm() {
    const form = $('#settings-form');
    if (!form) return;

    const account = await requireSession();
    if (!account) return;
    fillProfileForm(form, account.profile);
    initPersonalInferenceControls(form, account.profile);

    on(form, 'submit', async (event) => {
      event.preventDefault();
      try {
        const profile = await api('/api/me/profile', {
          method: 'PUT',
          body: JSON.stringify(buildProfilePayload(form))
        });
        state.account.profile = profile.profile;
        fillProfileForm(form, profile.profile);
        initPersonalInferenceControls(form, profile.profile);
        applyAccountChrome(state.account);
        toast('Settings saved');
      } catch (error) {
        toast(error.message);
      }
    });
  }

  async function initChat() {
    const form = $('#chat-form');
    const input = $('#chat-input');
    const log = $('#chat-messages');
    if (!form || !input || !log) return;

    const account = await requireSession();
    if (!account) return;
    if (!account.profile.onboarding_complete) {
      window.location.href = '/onboarding.html';
      return;
    }

    const inner = $('.chat-messages-inner', log) || log;
    const companionName = account.profile.companion_name || 'Hope';
    const userName = account.profile.user_name || 'You';

    function scrollToBottom() {
      log.scrollTop = log.scrollHeight;
    }

    function renderMessage(text, sender, createdAt) {
      const msg = document.createElement('div');
      msg.className = `message message--${sender}`;
      const initials = sender === 'assistant'
        ? companionName.charAt(0).toUpperCase()
        : userName.charAt(0).toUpperCase();
      const timeLabel = createdAt
        ? new Date(createdAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        : new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      msg.innerHTML = `
        <div class="message-avatar" aria-hidden="true">${escapeHtml(initials)}</div>
        <div>
          <div class="message-bubble">${escapeHtml(text)}</div>
          <div class="message-time">${timeLabel}</div>
        </div>`;
      inner.appendChild(msg);
      scrollToBottom();
    }

    function showTyping() {
      const el = document.createElement('div');
      el.className = 'message message--assistant';
      el.id = 'typing-msg';
      el.innerHTML = `
        <div class="message-avatar" aria-hidden="true">${escapeHtml(companionName.charAt(0).toUpperCase())}</div>
        <div class="typing-indicator" aria-label="${escapeHtml(companionName)} is typing">
          <span></span><span></span><span></span>
        </div>`;
      inner.appendChild(el);
      scrollToBottom();
    }

    function hideTyping() {
      $('#typing-msg')?.remove();
    }

    function autoResize() {
      input.style.height = 'auto';
      input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    }

    const history = await api('/api/chat/messages');
    if (history.messages.length === 0) {
      renderMessage(
        account.profile.user_name
          ? `Hi ${account.profile.user_name}. I'm ${companionName}. How are you feeling today?`
          : `Hi. I'm ${companionName}. How are you feeling today?`,
        'assistant'
      );
    } else {
      history.messages.forEach((message) => {
        renderMessage(message.content, message.role === 'assistant' ? 'assistant' : 'user', message.created_at);
      });
    }

    on(input, 'input', autoResize);
    on(input, 'keydown', (event) => {
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        form.requestSubmit();
      }
    });

    on(form, 'submit', async (event) => {
      event.preventDefault();
      const message = input.value.trim();
      if (!message) return;
      renderMessage(message, 'user');
      input.value = '';
      autoResize();
      showTyping();

      try {
        const response = await api('/api/chat', {
          method: 'POST',
          body: JSON.stringify({ message })
        });
        hideTyping();
        renderMessage(response.reply.content, 'assistant', response.reply.created_at);
      } catch (error) {
        hideTyping();
        toast(error.message);
      }
    });
  }

  async function initDangerActions() {
    const resetButton = $('#confirm-reset');
    const deleteButton = $('#confirm-delete');
    if (!resetButton && !deleteButton) return;

    await requireSession({ silent: true });

    on($('#btn-reset-bot'), 'click', () => $('#modal-reset')?.classList.add('open'));
    on($('#btn-delete-account'), 'click', () => $('#modal-delete')?.classList.add('open'));

    on(resetButton, 'click', async () => {
      try {
        await api('/api/me/reset', { method: 'POST' });
        $('#modal-reset')?.classList.remove('open');
        toast('Your companion has been reset');
        window.location.href = '/onboarding.html';
      } catch (error) {
        toast(error.message);
      }
    });

    on(deleteButton, 'click', async () => {
      try {
        await api('/api/me/account', { method: 'DELETE' });
        $('#modal-delete')?.classList.remove('open');
        toast('Your account was deleted');
        window.location.href = '/';
      } catch (error) {
        toast(error.message);
      }
    });
  }

  function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
  }

  async function init() {
    await initTenantSelectors();
    syncTenantLinks();
    populateTimezoneSelects();
    populateGeminiModelSelects();
    $$('.tabs').forEach((tabs) => initTabs(tabs.parentElement));
    initModals();
    await initLoginForm();
    await initOnboardingForm();
    await initSettingsForm();
    await initChat();
    await initDangerActions();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  return { toast };
})();
