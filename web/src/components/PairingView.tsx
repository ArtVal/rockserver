import { type PairingPreview } from "../api";
import { deviceName, deviceProductName, formatDate } from "./HardwareHud";

export type PairingState =
  | "loading"
  | "anonymous"
  | "authenticated"
  | "approving"
  | "approved"
  | "terminal"
  | "unavailable";

export interface RegistrationViewProps {
  registrationComplete: boolean;
  authenticatedAccountName: string;
  registrationName: string;
  authBusy: boolean;
  message: string;
  setRegistrationName: (name: string) => void;
  onUseDefaultName: () => void;
  onRegister: () => void;
  onReturn: () => void;
}

/** Renders the passkey registration screen. */
export function RegistrationView({
  registrationComplete,
  authenticatedAccountName,
  registrationName,
  authBusy,
  message,
  setRegistrationName,
  onUseDefaultName,
  onRegister,
  onReturn,
}: RegistrationViewProps) {
  return (
    <main>
      <header>
        <span>ROCK</span>
        <h1>Создать Rock-аккаунт</h1>
      </header>
      {registrationComplete ? (
        <section>
          <p className="eyebrow">Аккаунт создан</p>
          <h2>Вход в браузере выполнен</h2>
          <p>Rock-аккаунт «{authenticatedAccountName}» создан и защищён passkey.</p>
          <a className="button" href="/">
            Открыть аккаунт и устройства
          </a>
        </section>
      ) : (
        <section>
          <p>
            Создайте новый Rock-аккаунт с passkey. Для входа в существующий аккаунт используйте отдельный вход.
          </p>
          <label htmlFor="account-name">
            Имя аккаунта <span className="example">Например, Алексей</span>
          </label>
          <input
            id="account-name"
            value={registrationName}
            maxLength={128}
            placeholder="Например, Алексей"
            disabled={authBusy}
            onInput={event => setRegistrationName(event.currentTarget.value)}
          />
          <button
            className="secondary"
            onClick={onUseDefaultName}
            disabled={authBusy}
          >
            Использовать «Rock account»
          </button>
          <button onClick={onRegister} disabled={authBusy}>
            {authBusy ? "Создаём…" : "Создать аккаунт с passkey"}
          </button>
          <button
            className="link-button"
            onClick={onReturn}
            disabled={authBusy}
          >
            У меня уже есть аккаунт
          </button>
        </section>
      )}
      {message && <p role="alert">{message}</p>}
      <footer>Passkey и данные сессии не сохраняются в браузере.</footer>
    </main>
  );
}

export interface PairingViewProps {
  preview?: PairingPreview;
  pairingState: PairingState;
  authenticatedAccountName: string;
  csrf: string;
  authBusy: boolean;
  message: string;
  onApprove: () => void;
  onAuthenticate: () => void;
  onOpenRegistration: () => void;
  onOpenCabinet: () => void;
}

/** Renders the device pairing screen. */
export function PairingView({
  preview,
  pairingState,
  authenticatedAccountName,
  csrf,
  authBusy,
  message,
  onApprove,
  onAuthenticate,
  onOpenRegistration,
  onOpenCabinet,
}: PairingViewProps) {
  const deviceType = deviceProductName(preview?.device_type ?? "");
  return (
    <main>
      <header>
        <span>ROCK</span>
        <h1>Подключение устройства</h1>
      </header>
      {preview && pairingState !== "approved" && (
        <section>
          <p className="eyebrow">Проверьте, что это ваше устройство</p>
          <h2>{deviceName(preview)}</h2>
          <dl>
            <div>
              <dt>Проверочная фраза</dt>
              <dd aria-label={`Проверочная фраза: ${preview.verification_phrase}`}>
                {preview.verification_phrase}
              </dd>
            </div>
            <div>
              <dt>Короткий код</dt>
              <dd aria-label={`Короткий код: ${preview.short_code}`}>{preview.short_code}</dd>
            </div>
            <div>
              <dt>Действует до</dt>
              <dd>{formatDate(preview.expires_at)}</dd>
            </div>
          </dl>
        </section>
      )}
      {message && <p role="alert">{message}</p>}
      {preview && pairingState === "approved" ? (
        <section>
          <p className="eyebrow">✓ Устройство подключено</p>
          <h2>
            {deviceName(preview)} подключён к «{authenticatedAccountName}»
          </h2>
          {deviceType === "RockMobile" ? (
            <a className="button" href="/return/rockmobile">
              Вернуться в RockMobile
            </a>
          ) : (
            <p role="status">Вернитесь в RockCast или закройте браузер.</p>
          )}
          <button className="secondary" onClick={onOpenCabinet}>
            Открыть аккаунт и устройства
          </button>
        </section>
      ) : preview &&
        authenticatedAccountName &&
        csrf &&
        (pairingState === "authenticated" || pairingState === "approving") ? (
        <section>
          <h2>Подключить {deviceType} к аккаунту «{authenticatedAccountName}»?</h2>
          <p>Будет подключено только показанное выше устройство.</p>
          <button onClick={onApprove} disabled={pairingState !== "authenticated"}>
            {pairingState === "approving" ? "Подключаем…" : "Подключить"}
          </button>
          <a className="button secondary" href="/">
            Отмена
          </a>
        </section>
      ) : preview ? (
        <section>
          <h2>
            {authenticatedAccountName
              ? `Подтвердите вход в «${authenticatedAccountName}»`
              : "Чтобы продолжить"}
          </h2>
          <p>
            {authenticatedAccountName
              ? "Для подключения устройства требуется свежая проверка passkey."
              : "Войдите в существующий Rock-аккаунт или создайте новый. После этого вы вернётесь к этому устройству."}
          </p>
          <button onClick={onAuthenticate} disabled={authBusy}>
            {authBusy
              ? "Проверяем…"
              : authenticatedAccountName
              ? "Подтвердить passkey"
              : "Войти с passkey"}
          </button>
          {authenticatedAccountName ? (
            <>
              <p>
                Если passkey этого аккаунта удалён, восстановить его без сохранённого ключа нельзя.
              </p>
              <button className="secondary" onClick={onOpenRegistration} disabled={authBusy}>
                Создать другой Rock-аккаунт
              </button>
            </>
          ) : (
            <button className="secondary" onClick={onOpenRegistration} disabled={authBusy}>
              Создать Rock-аккаунт
            </button>
          )}
        </section>
      ) : (
        <section>
          <h2>Ссылка подключения недействительна</h2>
          <p>Откройте новую защищённую ссылку на устройстве, которое хотите подключить.</p>
        </section>
      )}
      <footer>Passkey и данные сессии не сохраняются в браузере.</footer>
    </main>
  );
}
