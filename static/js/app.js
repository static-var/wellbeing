'use strict';

const App = (() => {
  const state = {
    account: null,
    tenants: []
  };

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
    const isFormData = typeof FormData !== 'undefined' && options.body instanceof FormData;
    if (options.body && !isFormData && !headers.has('Content-Type')) {
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
      telegram_bot_username: null,
      personal_inference_enabled: true,
      personal_inference_model: null,
      personal_inference_api_key: optionalValue(data.personal_inference_api_key),
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
    const configured = Boolean(profile.personal_inference_api_key_configured);
    const keyInput = $('[name="personal_inference_api_key"]', form);
    const status = $('[data-personal-key-status]', form);
    if (keyInput) keyInput.required = !configured;

    if (!status) return;
    status.textContent = configured
      ? 'A Gemini key is already stored securely. Leave this blank to keep it, or paste a new one to rotate it.'
      : 'A Gemini key is required. It will be encrypted before it is stored.';
  }

  function initPersonalInferenceControls(form, profile = {}) {
    updatePersonalInferenceStatus(form, profile);
  }

  function setFormStatus(form, kind, message) {
    const status = $('[data-form-status]', form);
    if (!status) return;
    status.hidden = false;
    status.className = `form-status form-status--${kind}`;
    status.textContent = message;
  }

  function clearFormStatus(form) {
    const status = $('[data-form-status]', form);
    if (!status) return;
    status.hidden = true;
    status.className = 'form-status';
    status.textContent = '';
  }

  function setSubmitting(form, submitting, pendingLabel) {
    const submit = $('[type="submit"]', form);
    if (!submit) return;
    if (!submit.dataset.defaultLabel) {
      submit.dataset.defaultLabel = submit.textContent;
    }
    submit.disabled = submitting;
    submit.textContent = submitting ? pendingLabel : submit.dataset.defaultLabel;
  }

  function friendlySaveError(message) {
    if (message.includes('WELLBEING_MASTER_KEY')) {
      return 'This server is not configured to store Gemini keys yet. Set WELLBEING_MASTER_KEY on the server, restart Wellbeing, then save again.';
    }
    return message;
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
      clearFormStatus(form);
      setSubmitting(form, true, 'Saving...');
      try {
        const profile = await api('/api/me/profile', {
          method: 'PUT',
          body: JSON.stringify(buildProfilePayload(form))
        });
        state.account.profile = profile.profile;
        rememberTenantId(state.account.tenant_id);
        setFormStatus(form, 'success', 'Saved. Taking you to chat…');
        toast('You’re all set');
        window.location.href = '/chat.html';
      } catch (error) {
        const message = friendlySaveError(error.message);
        setFormStatus(form, 'error', message);
        toast(message, 5200);
      } finally {
        setSubmitting(form, false, 'Saving...');
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
      clearFormStatus(form);
      setSubmitting(form, true, 'Saving...');
      try {
        const profile = await api('/api/me/profile', {
          method: 'PUT',
          body: JSON.stringify(buildProfilePayload(form))
        });
        state.account.profile = profile.profile;
        fillProfileForm(form, profile.profile);
        initPersonalInferenceControls(form, profile.profile);
        applyAccountChrome(state.account);
        setFormStatus(form, 'success', 'Settings saved successfully.');
        toast('Settings saved');
      } catch (error) {
        const message = friendlySaveError(error.message);
        setFormStatus(form, 'error', message);
        toast(message, 5200);
      } finally {
        setSubmitting(form, false, 'Saving...');
      }
    });
  }

  async function initChat() {
    const form = $('#chat-form');
    const input = $('#chat-input');
    const log = $('#chat-messages');
    const newSessionButton = $('#chat-new-session');
    const sessionNote = $('#chat-session-note');
    const recordButton = $('#chat-record');
    const audioStatus = $('#chat-audio-status');
    const sendButton = $('.chat-send', form);
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
    const supportsAudioRecording = Boolean(window.MediaRecorder && navigator.mediaDevices?.getUserMedia);
    let isSending = false;
    let isRecording = false;
    let mediaRecorder = null;
    let mediaStream = null;
    let audioChunks = [];
    let autoStopTimer = null;

    function scrollToBottom() {
      log.scrollTop = log.scrollHeight;
    }

    function clearMessages() {
      inner.innerHTML = '';
    }

    function setSessionHint(text) {
      if (!sessionNote) return;
      const value = optionalValue(text);
      if (!value) {
        sessionNote.hidden = true;
        sessionNote.textContent = '';
        return;
      }
      sessionNote.textContent = value;
      sessionNote.hidden = false;
    }

    function setAudioStatus(message, stateName = 'idle') {
      if (!audioStatus) return;
      audioStatus.textContent = message;
      if (stateName === 'idle') {
        audioStatus.removeAttribute('data-state');
      } else {
        audioStatus.dataset.state = stateName;
      }
    }

    function clearAutoStopTimer() {
      if (autoStopTimer) {
        window.clearTimeout(autoStopTimer);
        autoStopTimer = null;
      }
    }

    function stopMediaStream() {
      if (mediaStream) {
        mediaStream.getTracks().forEach((track) => track.stop());
        mediaStream = null;
      }
    }

    function refreshComposerState() {
      const busy = isSending || isRecording;
      input.disabled = busy;
      if (sendButton) sendButton.disabled = busy;
      if (recordButton) {
        recordButton.disabled = isSending || !supportsAudioRecording;
        recordButton.classList.toggle('is-recording', isRecording);
        recordButton.setAttribute(
          'aria-label',
          isRecording ? 'Stop recording and send voice note' : 'Record a voice note'
        );
      }
    }

    function renderMessage(text, sender, createdAt) {
      const msg = document.createElement('div');
      msg.className = `message message--${sender}`;
      const initials = sender === 'assistant'
        ? companionName.charAt(0).toUpperCase()
        : userName.charAt(0).toUpperCase();
      const content = sender === 'assistant'
        ? renderAssistantMarkdown(text)
        : escapeHtml(text);
      const timeLabel = createdAt
        ? new Date(createdAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        : new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      msg.innerHTML = `
        <div class="message-avatar" aria-hidden="true">${escapeHtml(initials)}</div>
        <div>
          <div class="message-bubble">${content}</div>
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

    function preferredRecordingMimeType() {
      if (typeof MediaRecorder === 'undefined' || typeof MediaRecorder.isTypeSupported !== 'function') {
        return '';
      }

      const candidates = [
        'audio/webm;codecs=opus',
        'audio/webm',
        'audio/ogg;codecs=opus',
        'audio/mp4'
      ];
      return candidates.find((candidate) => MediaRecorder.isTypeSupported(candidate)) || '';
    }

    function recordedFileName(mimeType) {
      const normalized = String(mimeType || '').toLowerCase();
      if (normalized.includes('ogg')) return 'voice-note.ogg';
      if (normalized.includes('mp4') || normalized.includes('aac') || normalized.includes('m4a')) return 'voice-note.m4a';
      return 'voice-note.webm';
    }

    function friendlyMicError(error) {
      if (!error) return 'I could not access your microphone.';
      if (error.name === 'NotAllowedError' || error.name === 'SecurityError') {
        return 'Microphone permission was denied. Allow microphone access in your browser and try again.';
      }
      if (error.name === 'NotFoundError') {
        return 'No microphone was found on this device.';
      }
      if (error.name === 'NotReadableError') {
        return 'Your microphone is busy in another app right now. Please close that app and try again.';
      }
      return 'I could not start recording from your microphone.';
    }

    async function uploadAudioNote(blob, mimeType) {
      const formData = new FormData();
      formData.append('audio', blob, recordedFileName(mimeType));
      isSending = true;
      refreshComposerState();
      setAudioStatus('Transcribing your voice note…', 'success');
      showTyping();

      try {
        const response = await api('/api/chat/audio', {
          method: 'POST',
          body: formData
        });
        hideTyping();
        setSessionHint(response.session_hint);
        if (response.transcript) {
          renderMessage(response.transcript, 'user');
        }
        renderMessage(response.reply.content, 'assistant', response.reply.created_at);
        setAudioStatus('Voice note sent.', 'success');
      } catch (error) {
        hideTyping();
        const message = error.message || 'I could not send that voice note.';
        setAudioStatus(message, 'error');
        toast(message, 5200);
      } finally {
        isSending = false;
        refreshComposerState();
      }
    }

    async function startRecording() {
      if (!supportsAudioRecording) {
        const message = 'This browser does not support in-browser voice notes yet.';
        setAudioStatus(message, 'error');
        toast(message);
        return;
      }

      try {
        mediaStream = await navigator.mediaDevices.getUserMedia({ audio: true });
        const mimeType = preferredRecordingMimeType();
        mediaRecorder = mimeType
          ? new MediaRecorder(mediaStream, { mimeType })
          : new MediaRecorder(mediaStream);
        audioChunks = [];

        mediaRecorder.addEventListener('dataavailable', (event) => {
          if (event.data && event.data.size > 0) {
            audioChunks.push(event.data);
          }
        });

        mediaRecorder.addEventListener('stop', async () => {
          clearAutoStopTimer();
          stopMediaStream();
          isRecording = false;
          refreshComposerState();

          if (!audioChunks.length) {
            setAudioStatus('No audio was captured. Please try again.', 'error');
            return;
          }

          const actualMimeType = mediaRecorder?.mimeType || mimeType || 'audio/webm';
          const audioBlob = new Blob(audioChunks, { type: actualMimeType });
          audioChunks = [];
          await uploadAudioNote(audioBlob, actualMimeType);
        });

        mediaRecorder.addEventListener('error', () => {
          clearAutoStopTimer();
          stopMediaStream();
          isRecording = false;
          audioChunks = [];
          refreshComposerState();
          const message = 'Recording stopped unexpectedly. Please try again.';
          setAudioStatus(message, 'error');
          toast(message);
        });

        mediaRecorder.start();
        isRecording = true;
        refreshComposerState();
        setAudioStatus('Recording… tap the mic again to send.', 'recording');
        autoStopTimer = window.setTimeout(() => {
          if (mediaRecorder && mediaRecorder.state === 'recording') {
            mediaRecorder.stop();
          }
        }, 90000);
      } catch (error) {
        stopMediaStream();
        isRecording = false;
        refreshComposerState();
        const message = friendlyMicError(error);
        setAudioStatus(message, 'error');
        toast(message, 5200);
      }
    }

    function stopRecording() {
      if (mediaRecorder && mediaRecorder.state === 'recording') {
        setAudioStatus('Finishing your voice note…', 'success');
        mediaRecorder.stop();
      }
    }

    const history = await api('/api/chat/messages');
    if (supportsAudioRecording) {
      setAudioStatus('Voice notes are available here too. Tap the mic to record, then tap again to send.');
    } else {
      setAudioStatus('Voice notes are not available in this browser, but text chat still works normally.', 'error');
    }
    refreshComposerState();
    setSessionHint(history.session_hint);
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

    async function createFreshChat() {
      if (isSending || isRecording) return;
      showTyping();
      try {
        const response = await api('/api/chat/new', { method: 'POST' });
        hideTyping();
        clearMessages();
        setSessionHint(response.session_hint);
        renderMessage(response.reply.content, 'assistant', response.reply.created_at);
        toast('Started a fresh chat');
        input.focus();
      } catch (error) {
        hideTyping();
        toast(error.message);
      }
    }

    on(input, 'input', autoResize);
    on(input, 'keydown', (event) => {
      if (isSending || isRecording) {
        return;
      }
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        form.requestSubmit();
      }
    });

    on(recordButton, 'click', async () => {
      if (isSending) return;
      if (isRecording) {
        stopRecording();
      } else {
        await startRecording();
      }
    });

    on(form, 'submit', async (event) => {
      event.preventDefault();
      if (isSending || isRecording) return;
      const message = input.value.trim();
      if (!message) return;
      if (message === '/new') {
        input.value = '';
        autoResize();
        await createFreshChat();
        return;
      }
      renderMessage(message, 'user');
      input.value = '';
      autoResize();
      isSending = true;
      refreshComposerState();
      showTyping();

      try {
        const response = await api('/api/chat', {
          method: 'POST',
          body: JSON.stringify({ message })
        });
        hideTyping();
        setSessionHint(response.session_hint);
        renderMessage(response.reply.content, 'assistant', response.reply.created_at);
      } catch (error) {
        hideTyping();
        toast(error.message);
      } finally {
        isSending = false;
        refreshComposerState();
      }
    });

    on(newSessionButton, 'click', createFreshChat);
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

  function sanitizeHref(url) {
    try {
      const parsed = new URL(url, window.location.origin);
      if (parsed.protocol === 'http:' || parsed.protocol === 'https:') {
        return parsed.href;
      }
    } catch (_) {
      return null;
    }
    return null;
  }

  function renderInlineMarkdown(text) {
    let html = escapeHtml(text);
    html = html.replace(/`([^`\n]+)`/g, '<code>$1</code>');
    html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    html = html.replace(/(^|[^\*])\*([^*\n]+)\*/g, '$1<em>$2</em>');
    html = html.replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, (_, label, href) => {
      const safeHref = sanitizeHref(href);
      const safeLabel = label;
      if (!safeHref) return safeLabel;
      return `<a href="${escapeHtml(safeHref)}" target="_blank" rel="noopener noreferrer">${safeLabel}</a>`;
    });
    return html;
  }

  function renderAssistantMarkdown(text) {
    const normalized = String(text || '').replace(/\r\n/g, '\n').trim();
    if (!normalized) return '';

    const blocks = normalized.split(/\n{2,}/).map((block) => block.trim()).filter(Boolean);
    const htmlBlocks = blocks.map((block) => {
      const lines = block.split('\n').map((line) => line.trim()).filter(Boolean);
      if (!lines.length) return '';

      if (lines.every((line) => /^[-*]\s+/.test(line))) {
        const items = lines
          .map((line) => `<li>${renderInlineMarkdown(line.replace(/^[-*]\s+/, ''))}</li>`)
          .join('');
        return `<ul>${items}</ul>`;
      }

      if (lines.every((line) => /^\d+\.\s+/.test(line))) {
        const items = lines
          .map((line) => `<li>${renderInlineMarkdown(line.replace(/^\d+\.\s+/, ''))}</li>`)
          .join('');
        return `<ol>${items}</ol>`;
      }

      const headingMatch = lines.length === 1 ? lines[0].match(/^(#{1,3})\s+(.+)$/) : null;
      if (headingMatch) {
        const level = Math.min(headingMatch[1].length + 2, 4);
        return `<h${level}>${renderInlineMarkdown(headingMatch[2])}</h${level}>`;
      }

      return `<p>${lines.map(renderInlineMarkdown).join('<br>')}</p>`;
    }).filter(Boolean);

    return htmlBlocks.join('');
  }

  async function init() {
    await initTenantSelectors();
    syncTenantLinks();
    populateTimezoneSelects();
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
