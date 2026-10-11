import { useEffect, useRef, useState } from "react";
import { Button, Field, Input, Switch } from "@fluentui/react-components";
import { api } from "../../api";
import type { AppSettings } from "../../types";

type Sms = { phone: string; png: string; code: string; graphic: string; trust: boolean };

export function Login(props: {
  settings: AppSettings | null;
  setSettings: (s: AppSettings) => void;
  user: string;
  setUser: (v: string) => void;
  pass: string;
  setPass: (v: string) => void;
  remember: boolean;
  setRemember: (v: boolean) => void;
  autoLogin: boolean;
  setAutoLogin: (v: boolean) => void;
  step: "login" | "sms";
  setStep: (v: "login" | "sms") => void;
  sms: Sms;
  setSms: (v: Sms | ((s: Sms) => Sms)) => void;
  onLoggedIn: (username?: string) => void;
  onSkip: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [smsWait, setSmsWait] = useState(0);
  const [smsSending, setSmsSending] = useState(false);
  const [captchaBusy, setCaptchaBusy] = useState(false);
  const [smsHint, setSmsHint] = useState("");
  const loginLock = useRef(false);
  const smsLock = useRef(false);
  const { settings } = props;

  useEffect(() => {
    if (smsWait <= 0) return;
    const t = window.setTimeout(() => setSmsWait((n) => Math.max(0, n - 1)), 1000);
    return () => window.clearTimeout(t);
  }, [smsWait]);

  function patchRemember(next: boolean) {
    props.setRemember(next);
    if (!next) {
      props.setAutoLogin(false);
      props.setPass("");
    }
    if (settings) props.setSettings({ ...settings, rememberPassword: next, autoLogin: next && props.autoLogin });
  }

  function patchAuto(next: boolean) {
    if (!props.remember) return;
    props.setAutoLogin(next);
    if (settings) props.setSettings({ ...settings, autoLogin: next });
  }

  async function doLogin() {
    if (loginLock.current || busy) return;
    if (!props.user.trim() || !props.pass) {
      setErr("请输入学号和密码");
      return;
    }
    loginLock.current = true;
    setBusy(true);
    setErr("");
    try {
      const out = await api.login(props.user.trim(), props.pass, props.remember, props.autoLogin);
      if (out.status === "need_sms") {
        props.setSms({
          phone: out.phoneMask,
          png: out.captchaPng,
          graphic: "",
          code: "",
          trust: true,
        });
        setSmsWait(0);
        setSmsHint("");
        props.setStep("sms");
      } else if (out.status === "ok") {
        if (!props.remember) props.setPass("");
        props.onLoggedIn(props.user.trim());
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
      loginLock.current = false;
    }
  }

  async function refreshCaptcha() {
    if (captchaBusy) return;
    setCaptchaBusy(true);
    setErr("");
    try {
      const png = await api.refreshCaptcha();
      props.setSms((s) => ({ ...s, png, graphic: "" }));
    } catch (e) {
      setErr(String(e));
    } finally {
      setCaptchaBusy(false);
    }
  }

  async function sendSmsCode() {
    if (smsSending || smsWait > 0 || captchaBusy) return;
    if (!props.sms.graphic.trim()) {
      setErr("请输入图形验证码");
      return;
    }
    setSmsSending(true);
    setErr("");
    setSmsHint("");
    try {
      await api.sendSms(props.sms.graphic.trim());
      setSmsWait(60);
      setSmsHint("验证码发送成功，请注意查收");
    } catch (e) {
      const msg = String(e);
      setErr(msg);
      const graphicWrong =
        /请输入图形验证码|图形码不对|图形验证码/.test(msg)
        || (/验证码错误|验证码有误|验证码不正确/.test(msg) && !/短信|动态|身份验证/.test(msg));
      if (graphicWrong) await refreshCaptcha();
    } finally {
      setSmsSending(false);
    }
  }

  async function submitSms() {
    if (smsLock.current || busy) return;
    if (!/^\d{6}$/.test(props.sms.code.trim())) {
      setErr("请输入 6 位短信码");
      return;
    }
    smsLock.current = true;
    setBusy(true);
    setErr("");
    try {
      const out = await api.submitSms(props.sms.code.trim(), props.sms.trust);
      if (out.status !== "ok") {
        setErr("短信码不对，请重新获取后再试");
        return;
      }
      if (!props.remember) props.setPass("");
      props.onLoggedIn();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
      smsLock.current = false;
    }
  }

  if (props.step === "sms") {
    return (
      <div className="login-wrap">
        <div className="login-card">
          <h1 className="page-title">短信验证</h1>
          <p className="hint">绑定手机 {props.sms.phone}。这是登录的下一步。</p>
          <Field label="图形验证码 · 学校的，不是泥洞的">
            <div className="sms-row">
              <Input
                autoComplete="off"
                value={props.sms.graphic}
                placeholder="图形码"
                onChange={(_, d) => props.setSms((s) => ({ ...s, graphic: d.value }))}
                onKeyDown={(e) => e.key === "Enter" && sendSmsCode()}
              />
              <img
                className="captcha"
                src={props.sms.png ? `data:image/png;base64,${props.sms.png}` : undefined}
                alt="图形验证码"
                title="点击换一张"
                onClick={() => refreshCaptcha()}
              />
            </div>
          </Field>
          <Button appearance="subtle" disabled={captchaBusy} onClick={() => refreshCaptcha()}>
            {captchaBusy ? "正在换图…" : "看不清，换一张"}
          </Button>
          <Field label="短信码">
            <div className="sms-row">
              <Input
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={6}
                placeholder="6 位"
                value={props.sms.code}
                onChange={(_, d) => props.setSms((s) => ({ ...s, code: d.value.replace(/\D/g, "").slice(0, 6) }))}
                onKeyDown={(e) => e.key === "Enter" && submitSms()}
              />
              <Button appearance="subtle" disabled={smsSending || smsWait > 0 || captchaBusy} onClick={() => sendSmsCode()}>
                {smsSending ? "发送中…" : smsWait > 0 ? `${smsWait} 秒` : "发送"}
              </Button>
            </div>
          </Field>
          {smsHint ? <p className="hint">{smsHint}</p> : null}
          <Switch label="信任这台电脑" checked={props.sms.trust} onChange={(_, d) => props.setSms((s) => ({ ...s, trust: d.checked }))} />
          <Button appearance="primary" disabled={busy || props.sms.code.trim().length !== 6} onClick={() => submitSms()}>
            {busy ? "登录中…" : "继续"}
          </Button>
          <Button
            appearance="subtle"
            onClick={async () => {
              setErr("");
              setSmsWait(0);
              setSmsHint("");
              props.setSms((s) => ({ ...s, code: "", graphic: "", png: "" }));
              try { await api.cancelSms(); } catch { /* ignore */ }
              props.setStep("login");
            }}
          >
            返回登录
          </Button>
          {err ? <p className="hint warn">{err}</p> : null}
        </div>
      </div>
    );
  }

  return (
    <div className="login-wrap">
      <div className="login-card">
        <h1 className="page-title">进入泥洞</h1>
        <Field label="学号">
          <Input
            name="username"
            autoComplete="username"
            inputMode="numeric"
            placeholder="一号通学号"
            spellCheck={false}
            value={props.user}
            onChange={(_, d) => props.setUser(d.value)}
          />
        </Field>
        <Field label="密码">
          <Input
            type="password"
            name="password"
            autoComplete="current-password"
            placeholder="一号通密码"
            value={props.pass}
            onChange={(_, d) => props.setPass(d.value)}
            onKeyDown={(e) => e.key === "Enter" && doLogin()}
          />
        </Field>
        <Switch label="记住密码" checked={props.remember} onChange={(_, d) => patchRemember(d.checked)} />
        <Switch label="自动登录" disabled={!props.remember} checked={props.autoLogin && props.remember} onChange={(_, d) => patchAuto(d.checked)} />
        <Button appearance="primary" disabled={busy} onClick={doLogin}>{busy ? "登录中…" : "进入泥洞"}</Button>
        <Button appearance="subtle" disabled={busy} onClick={props.onSkip}>跳过登录</Button>
        {err ? <p className="hint warn">{err}</p> : null}
        <p className="hint">跳过后可以使用工具箱、日程、任意门和设置。校园卡、网费、电费和课程需要登录。</p>
        <p className="hint">勾选后密码存进这台电脑的系统凭据，下次打开会自动填入。关掉「记住密码」会删掉已存的密码。退出登录不会清。</p>
      </div>
    </div>
  );
}

export type { Sms };
