// Shared UI state: the face library and the face being edited.

import * as api from "./api";
import { defaultFace, normalizeFace, type FaceDef } from "./types";

export type FaceSource = { kind: "preset" | "saved"; name: string };

export function sameFace(a: FaceDef, b: FaceDef): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

class AppState {
  presets = $state<FaceDef[]>([]);
  saved = $state<FaceDef[]>([]);
  /** Working copy shown in the simulator and editor. */
  face = $state<FaceDef>(defaultFace());
  /** Library entry the working copy came from. */
  source = $state<FaceSource | null>(null);
  loadError = $state<string | null>(null);

  sourceFace = $derived.by(() => {
    const s = this.source;
    if (!s) return null;
    return (s.kind === "preset" ? this.presets : this.saved).find((f) => f.name === s.name) ?? null;
  });

  dirty = $derived(this.sourceFace ? !sameFace(this.sourceFace, this.face) : true);

  /** A saved face already uses the working copy's name (save overwrites it). */
  savedMatch = $derived(this.saved.find((f) => f.name === this.face.name) ?? null);

  async load() {
    try {
      const [presets, saved] = await Promise.all([api.listPresets(), api.listSavedFaces()]);
      this.presets = presets;
      this.saved = saved;
      if (!this.source && presets.length) this.select("preset", presets[0]);
    } catch (e) {
      this.loadError = api.errorText(e);
    }
  }

  select(kind: FaceSource["kind"], face: FaceDef) {
    this.face = { ...face };
    this.source = { kind, name: face.name };
  }

  isSelected(kind: FaceSource["kind"], name: string): boolean {
    return this.source?.kind === kind && this.source.name === name;
  }

  async save() {
    const face = $state.snapshot(this.face);
    await api.saveFace(face);
    this.saved = await api.listSavedFaces();
    // The backend normalises names; follow whatever it stored.
    const name = face.name.trim();
    const stored = this.saved.find((f) => f.name === name)?.name ?? name;
    if (this.face.name !== stored) this.face.name = stored;
    this.source = { kind: "saved", name: stored };
  }

  async remove(name: string) {
    await api.deleteFace(name);
    this.saved = await api.listSavedFaces();
    if (this.isSelected("saved", name)) this.source = null;
  }

  /** Loads an imported face (already validated by the backend) as an unsaved working copy. */
  loadImported(face: FaceDef) {
    this.face = normalizeFace(face);
    this.source = null;
  }
}

export const app = new AppState();
