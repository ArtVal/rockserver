import { useEffect, useRef, useState } from "preact/hooks";
import {
  api,
  browserAuthenticationOptions,
  browserRegistrationOptions,
  serializeAuthentication,
  serializeRegistration,
  type ApiError,
  type BrowserAccount,
  type BrowserDevice,
  type PairingPreview,
} from "./api";
import { AdminApp } from "./admin";
import { AccountCentre, type AccountState } from "./components/AccountCentre";
import { PairingView, RegistrationView, type PairingState } from "./components/PairingView";
import { deviceName, type JustConnected } from "./components/HardwareHud";
import "./style.css";

const errorMessage = (error: unknown) => {
  const code = (error as ApiError)?.code;
  if (code === "pairing_not_found" || code === "pairing_not_approvable")
    return "Этот запрос на подключение истёк, уже завершён или устройство уже подключено.";
  if (code === "auth_unavailable" || code === "pairing_unavailable" || code === "server_unavailable")
    return "Сервер временно недоступен. Повторите попытку позже.";
  return "Не удалось выполнить действие. Повторите попытку позже.";
};

const passkeyErrorMessage = (error: unknown) => {
  if (error instanceof DOMException) {
    if (error.name === "AbortError") return "Вход отменён пользователем.";
    if (error.name === "NotAllowedError")
      return "Ключ не найден. Выберите passkey этого Rock-аккаунта и попробуйте ещё раз.";
  }
  if ((error as ApiError)?.code === "webauthn_rejected")
    return "Ключ не найден. Выберите passkey этого Rock-аккаунта и попробуйте ещё раз.";
  return (error as ApiError)?.code
    ? "Сервер временно недоступен. Повторите попытку позже."
    : "Не удалось выполнить вход с passkey.";
};

const registrationErrorMessage = (error: unknown) =>
  error instanceof DOMException &&
  (error.name === "AbortError" || error.name === "NotAllowedError")
    ? "Создание отменено. Имя аккаунта сохранено — попробуйте ещё раз, когда будете готовы."
    : errorMessage(error);

const isAuthenticationError = (error: unknown) =>
  (error as ApiError)?.code === "authentication_required";

const LEGACY_QUERY_SECRET_ROLLOUT_END = Date.parse("2026-09-29T00:00:00Z");

/** Selects the same-origin administrator SPA without changing public browser routes. */
export function App() {
  return location.pathname === "/admin" ? <AdminApp /> : <PublicApp />;
}

/** Renders the account landing page or the secure, request-specific pairing screen. */
function PublicApp() {
  const params = new URLSearchParams(location.search);
  const parsedCode = params.get("code")?.trim().toUpperCase() ?? "";
  const fragment = new URLSearchParams(location.hash.slice(1));
  const fragmentSecret = fragment.get("secret") ?? "";
  const legacySecret = params.get("secret") ?? "";
  const yandexHomeParam = params.get("yandex_home") ?? "";

  // Keep the old query handoff only through the bounded rollout window.
  const parsedApprovalSecret =
    fragmentSecret || (Date.now() <= LEGACY_QUERY_SECRET_ROLLOUT_END ? legacySecret : "");
  if (fragmentSecret || legacySecret) {
    params.delete("secret");
    history.replaceState(null, "", `${location.pathname}${params.size ? `?${params}` : ""}`);
  }
  if (yandexHomeParam) {
    params.delete("yandex_home");
    history.replaceState(null, "", `${location.pathname}${params.size ? `?${params}` : ""}`);
  }

  const [yandexHomeStatus] = useState(yandexHomeParam);
  const handoff = useRef({
    code: parsedCode,
    approvalSecret: parsedApprovalSecret,
    pairingSearch: params.size ? `?${params}` : "",
  });
  const { code, approvalSecret: inMemoryApprovalSecret, pairingSearch } = handoff.current;
  const approvalSecret = inMemoryApprovalSecret;
  const isPairing = Boolean(code && approvalSecret);

  const [screen, setScreen] = useState<"main" | "register">(() =>
    location.pathname === "/register" ? "register" : "main"
  );
  const [preview, setPreview] = useState<PairingPreview>();
  const [message, setMessage] = useState("");
  const [accountMessage, setAccountMessage] = useState("");
  const [csrf, setCsrf] = useState("");
  const [registrationName, setRegistrationName] = useState("");
  const [authenticatedAccountName, setAuthenticatedAccountName] = useState("");
  const [pairingState, setPairingState] = useState<PairingState>("loading");
  const [accountState, setAccountState] = useState<AccountState>("loading");
  const [authBusy, setAuthBusy] = useState(false);
  const [deviceBusy, setDeviceBusy] = useState("");
  const [logoutBusy, setLogoutBusy] = useState(false);
  const [account, setAccount] = useState<BrowserAccount>();
  const [registrationComplete, setRegistrationComplete] = useState(false);
  const [showCabinet, setShowCabinet] = useState(false);
  const [justConnected, setJustConnected] = useState<JustConnected>();
  const registrationBusy = useRef(false);
  const approveBusy = useRef(false);

  const lookup = async () => {
    try {
      setPreview(await api.pairing(code));
    } catch (error) {
      setPreview(undefined);
      setPairingState(
        (error as ApiError)?.code === "server_unavailable" ? "unavailable" : "terminal"
      );
      setMessage(errorMessage(error));
    }
  };

  const loadAccount = async (expiredOnUnauthorized = false) => {
    try {
      const session = await api.browserSession();
      const nextAccount = await api.browserAccount();
      setCsrf(session.csrf_token);
      setAuthenticatedAccountName(session.account_display_name);
      setAccount(nextAccount);
      setAccountState("authenticated");
    } catch (error) {
      setAccount(undefined); setCsrf(""); setAuthenticatedAccountName("");
      if (isAuthenticationError(error))
        setAccountState(expiredOnUnauthorized ? "expired" : "anonymous");
      else {
        setAccountState("unavailable");
        setAccountMessage(errorMessage(error));
      }
    }
  };

  const restoreSession = async () => {
    if (!isPairing || showCabinet) {
      await loadAccount();
      return;
    }
    try {
      const session = await api.browserSession();
      setCsrf(session.csrf_token);
      setAuthenticatedAccountName(session.account_display_name);
      // Keep terminal and in-flight states if the session response arrives late.
      setPairingState(current => current === "approved" || current === "approving" || current === "terminal" || current === "unavailable" ? current : "authenticated");
    } catch (error) {
      if (isAuthenticationError(error))
        setPairingState(current => current === "loading" ? "anonymous" : current);
      else {
        setPairingState("unavailable");
        setMessage(errorMessage(error));
      }
    }
  };

  useEffect(() => {
    void restoreSession();
    if (isPairing && !showCabinet) void lookup();
  }, [showCabinet]);

  const register = async () => {
    if (registrationBusy.current) return;
    if (!registrationName.trim()) {
      setMessage("Введите имя аккаунта или выберите «Rock account».");
      return;
    }
    registrationBusy.current = true;
    setAuthBusy(true);
    setMessage("");
    try {
      if (!window.PublicKeyCredential) throw new Error("Браузер не поддерживает passkey.");
      const started = await api.registrationOptions({ account_display_name: registrationName.trim() });
      const credential = await navigator.credentials.create({
        publicKey: browserRegistrationOptions(started),
      });
      if (!(credential instanceof PublicKeyCredential)) throw new Error("Passkey не создан.");
      const result = await api.registrationVerify({
        challenge_id: started.challenge_id,
        ...serializeRegistration(credential),
      });
      setCsrf(result.csrf_token);
      setAuthenticatedAccountName(result.account_display_name);
      if (isPairing) {
        history.replaceState(null, "", `/${pairingSearch}`);
        setScreen("main");
        setMessage(
          "Rock-аккаунт создан, вход выполнен. Теперь подтвердите показанное устройство."
        );
        await lookup();
        setPairingState("authenticated");
      } else {
        setRegistrationComplete(true);
      }
    } catch (error) {
      setMessage(registrationErrorMessage(error));
    } finally {
      registrationBusy.current = false;
      setAuthBusy(false);
    }
  };

  const authenticate = async () => {
    if (!window.PublicKeyCredential) {
      setMessage("Браузер не поддерживает passkey.");
      return;
    }
    setAuthBusy(true);
    setMessage("");
    try {
      const started = await api.authenticationOptions();
      const credential = await navigator.credentials.get({
        publicKey: browserAuthenticationOptions(started),
      });
      if (!(credential instanceof PublicKeyCredential)) {
        setMessage("Ключ не найден. Выберите passkey этого Rock-аккаунта и попробуйте ещё раз.");
        return;
      }
      const result = await api.authenticationVerify({
        challenge_id: started.challenge_id,
        ...serializeAuthentication(credential),
      });
      setCsrf(result.csrf_token);
      if (isPairing && !showCabinet) {
        await lookup();
        const session = await api.browserSession();
        setCsrf(session.csrf_token);
        setAuthenticatedAccountName(session.account_display_name);
        setPairingState("authenticated");
      } else {
        await loadAccount();
      }
      setMessage(
        isPairing && !showCabinet
          ? "Вход выполнен. Проверьте устройство перед подключением."
          : "Вход выполнен."
      );
    } catch (error) {
      setMessage(passkeyErrorMessage(error));
    } finally {
      setAuthBusy(false);
    }
  };

  const approve = async () => {
    // The ref guard blocks a second in-flight confirmation even before the state re-render.
    if (approveBusy.current) return;
    if (!preview || !authenticatedAccountName || !csrf || pairingState !== "authenticated") {
      setMessage("Сначала войдите с passkey.");
      return;
    }
    approveBusy.current = true;
    setAuthBusy(true);
    setPairingState("approving");
    setMessage("");
    try {
      await api.approvePairing(
        preview.request_id,
        approvalSecret,
        preview.verification_phrase,
        csrf
      );
      setPairingState("approved");
    } catch (error) {
      const code = (error as ApiError)?.code;
      if (code === "auth_unavailable" || code === "server_unavailable") {
        // The server could not be reached; the request itself stays approvable for a retry.
        setPairingState("authenticated");
      } else {
        setPairingState("terminal");
      }
      setMessage(errorMessage(error));
    } finally {
      approveBusy.current = false;
      setAuthBusy(false);
    }
  };

  /** Local cancellation of the shown request; the pairing link itself stays unapproved. */
  const cancelPairing = () => {
    setPairingState("cancelled");
    setMessage("");
  };

  /** Re-runs the pairing lookup and session restore after a server-unavailable outcome. */
  const retryPairing = () => {
    setMessage("");
    setPairingState("loading");
    void lookup();
    void restoreSession();
  };

  const refreshAccount = async () => {
    setAccountState("loading");
    setAccountMessage("");
    await loadAccount(true);
  };

  const rename = async (device: BrowserDevice) => {
    const name = window.prompt("Новое имя устройства", device.device_display_name)?.trim();
    if (!name || name === device.device_display_name) return;
    setDeviceBusy(device.device_id);
    setAccountMessage("");
    try {
      await api.renameDevice(device.device_id, name, csrf);
      await refreshAccount();
      setAccountMessage("Имя устройства обновлено.");
    } catch (error) {
      if (isAuthenticationError(error)) {
        setAccount(undefined);
        setAccountState("expired");
      } else setAccountMessage(errorMessage(error));
    } finally {
      setDeviceBusy("");
    }
  };

  const revoke = async (device: BrowserDevice) => {
    const name = deviceName(device);
    if (
      !window.confirm(
        `Отключить «${name}»? На нём будет завершён вход в RockCast или RockMobile. Это не завершит вход в текущем браузере.`
      )
    )
      return;
    setDeviceBusy(device.device_id);
    setAccountMessage("");
    try {
      await api.revokeDevice(device.device_id, csrf);
      await refreshAccount();
      setAccountMessage("Устройство отключено.");
    } catch (error) {
      if (isAuthenticationError(error)) {
        setAccount(undefined);
        setAccountState("expired");
      } else setAccountMessage(errorMessage(error));
    } finally {
      setDeviceBusy("");
    }
  };

  const logout = async () => {
    if (!window.confirm("Выйти из Rock-аккаунта в этом браузере? Устройства останутся подключёнными."))
      return;
    setLogoutBusy(true);
    setAccountMessage("");
    try {
      await api.logoutBrowser(csrf);
      setCsrf("");
      setAuthenticatedAccountName("");
      setAccount(undefined);
      setAccountState("anonymous");
      setAccountMessage("Вы вышли из аккаунта в этом браузере.");
    } catch (error) {
      if (isAuthenticationError(error)) {
        setAccount(undefined);
        setAccountState("expired");
      } else setAccountMessage(errorMessage(error));
    } finally {
      setLogoutBusy(false);
    }
  };

  const openRegistration = () => {
    history.pushState(null, "", `/register${pairingSearch}`);
    setMessage("");
    setScreen("register");
  };

  const returnFromRegistration = () => {
    history.replaceState(null, "", `/${pairingSearch}`);
    setMessage("");
    setScreen("main");
  };

  const openCabinet = () => {
    if (preview)
      setJustConnected({
        device_display_name: preview.device_display_name,
        device_type: preview.device_type,
      });
    history.replaceState(null, "", "/");
    setShowCabinet(true);
    setAccountState("loading");
  };

  if (screen === "register") {
    return (
      <RegistrationView
        registrationComplete={registrationComplete}
        authenticatedAccountName={authenticatedAccountName}
        registrationName={registrationName}
        authBusy={authBusy}
        message={message}
        setRegistrationName={setRegistrationName}
        onUseDefaultName={() => setRegistrationName("Rock account")}
        onRegister={register}
        onReturn={returnFromRegistration}
      />
    );
  }

  if (!isPairing || showCabinet) {
    return (
      <AccountCentre
        account={account}
        accountState={accountState}
        accountName={authenticatedAccountName}
        accountMessage={accountMessage || message}
        csrf={csrf}
        authBusy={authBusy}
        deviceBusy={deviceBusy}
        logoutBusy={logoutBusy}
        initialTab={justConnected ? "devices" : "stations"}
        justConnected={justConnected}
        yandexHomeStatus={yandexHomeStatus}
        onAuthenticate={authenticate}
        onRegister={openRegistration}
        onRetry={refreshAccount}
        onRename={rename}
        onRevoke={revoke}
        onLogout={logout}
      />
    );
  }

  return (
    <PairingView
      preview={preview}
      pairingState={pairingState}
      authenticatedAccountName={authenticatedAccountName}
      authBusy={authBusy}
      message={message}
      onApprove={approve}
      onCancel={cancelPairing}
      onRetry={retryPairing}
      onAuthenticate={authenticate}
      onOpenRegistration={openRegistration}
      onOpenCabinet={openCabinet}
    />
  );
}
