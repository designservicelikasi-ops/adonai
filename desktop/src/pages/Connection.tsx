import { useEffect, useState } from "react";
import { api, LocalStatus, ProfileView } from "../api";
import { Badge, Button, Panel, useToast } from "../components/ui";

export default function Connection({ profile, onConnected, onForget }: { profile: ProfileView | null; onConnected: (p: ProfileView) => void; onForget: () => void }) {
  const toast = useToast();
  const [local, setLocal] = useState<LocalStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [url, setUrl] = useState(profile?.url ?? "https://");
  const [token, setToken] = useState("");
  const [fp, setFp] = useState(profile?.fingerprint ?? "");
  const [probed, setProbed] = useState<string | null>(null);

  useEffect(() => {
    api.localStatus().then(setLocal).catch(() => setLocal(null));
  }, []);

  const startLocal = async () => {
    setBusy(true);
    try {
      await api.localStart();
      toast("ok", "Moteur local démarré et appairé. Il continue de tourner si vous fermez l'application.");
      onConnected({ url: "https://127.0.0.1:7879", fingerprint: "", has_token: true, local: true });
    } catch (e) {
      toast("bad", String((e as Error).message));
    } finally {
      setBusy(false);
      api.localStatus().then(setLocal).catch(() => undefined);
    }
  };

  const probe = async () => {
    setBusy(true);
    try {
      const f = await api.probe(url);
      setProbed(f);
    } catch (e) {
      toast("bad", String((e as Error).message));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    try {
      await api.profileSave(url, token, fp);
      toast("ok", "Connexion établie : certificat vérifié, jeton accepté.");
      onConnected({ url, fingerprint: fp, has_token: true, local: false });
    } catch (e) {
      toast("bad", String((e as Error).message));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mx-auto grid max-w-5xl gap-5 lg:grid-cols-2">
      <div className="lg:col-span-2">
        <h1 className="text-xl font-semibold">Connexion au moteur</h1>
        <p className="mt-1 text-sm text-fog-400">
          AEGIS Desktop pilote un moteur AEGIS : sur cet ordinateur, ou sur un VPS. Le moteur tourne indépendamment de cette fenêtre ; aucun ordre ne part tant qu'un mode automatique n'est pas activé explicitement.
        </p>
      </div>
      <Panel title="Moteur local" right={local && <Badge tone={local.running ? "ok" : local.installed ? "muted" : "bad"}>{local.running ? "en marche" : local.installed ? "arrêté" : "absent"}</Badge>}>
        <p className="mb-4 text-sm text-fog-300">
          Démarre le moteur installé avec l'application, en processus détaché, avec une configuration et un certificat propres à cette machine, puis s'y connecte automatiquement.
        </p>
        <div className="mb-4 text-xs text-fog-500 num break-all">{local?.data_dir}</div>
        {local && !local.installed && <p className="mb-3 text-xs text-down-400">{local.detail}</p>}
        <Button kind="primary" disabled={busy || (local !== null && !local.installed)} onClick={startLocal}>
          {local?.running ? "Se connecter au moteur local" : "Démarrer le moteur local"}
        </Button>
      </Panel>
      <Panel title="Moteur distant (VPS)">
        <div className="space-y-3 text-sm">
          <label className="block">
            <span className="text-xs text-fog-400">Adresse</span>
            <input className="mt-1 w-full num" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://203.0.113.10:7879" />
          </label>
          <label className="block">
            <span className="text-xs text-fog-400">Jeton (affiché par « aegisd pairing » sur le serveur)</span>
            <input className="mt-1 w-full num" type="password" value={token} onChange={(e) => setToken(e.target.value)} />
          </label>
          <label className="block">
            <span className="text-xs text-fog-400">Empreinte SHA-256 du certificat</span>
            <input className="mt-1 w-full num text-xs" value={fp} onChange={(e) => setFp(e.target.value)} placeholder="ab:cd:…" />
          </label>
          {probed && (
            <div className="rounded-md border border-warn-400/30 bg-warn-400/5 p-3 text-xs">
              <div className="mb-1 text-warn-400">Empreinte présentée par le serveur :</div>
              <div className="num break-all text-fog-100">{probed}</div>
              <div className="mt-2 text-fog-400">Comparez-la avec celle affichée sur le serveur par « aegisd pairing ». Ne l'acceptez que si elles sont identiques.</div>
              <Button small kind="primary" onClick={() => setFp(probed)}>Elles sont identiques : utiliser</Button>
            </div>
          )}
          <div className="flex gap-2 pt-1">
            <Button disabled={busy || !url.startsWith("https://")} onClick={probe}>Lire l'empreinte</Button>
            <Button kind="primary" disabled={busy || !token || !fp} onClick={save}>Se connecter</Button>
          </div>
        </div>
      </Panel>
      {profile && (
        <Panel title="Connexion enregistrée" className="lg:col-span-2">
          <div className="flex items-center justify-between text-sm">
            <span className="num text-fog-300">{profile.url} {profile.local ? "(local)" : ""}</span>
            <Button
              kind="ghost"
              onClick={async () => {
                await api.profileForget();
                onForget();
              }}
            >
              Oublier cette connexion
            </Button>
          </div>
        </Panel>
      )}
    </div>
  );
}
