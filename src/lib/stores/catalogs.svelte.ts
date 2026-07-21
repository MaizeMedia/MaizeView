// Shared cache for the tag/performer/studio catalogs. These lists change
// rarely, but every drawer open / panel mount used to refetch all three.
// Svelte 5 runes ($state) in a .svelte.ts module — the supported way to
// share reactive state outside components.

import {
  performers as performersApi,
  studios as studiosApi,
  tags as tagsApi,
} from "$lib/api";
import type { PerformerRow, StudioRow, TagRow } from "$lib/api/types";

const byName = <T extends { name: string }>(a: T, b: T): number =>
  a.name.localeCompare(b.name);

class CatalogsStore {
  tags = $state<TagRow[]>([]);
  performers = $state<PerformerRow[]>([]);
  studios = $state<StudioRow[]>([]);
  /** True once the first successful fetch landed. */
  loaded = $state(false);
  /** Single in-flight load shared by concurrent ensureLoaded() callers. */
  private loadPromise: Promise<void> | null = null;

  /** Idempotent: fetches once, then no-ops; concurrent callers share the fetch. */
  async ensureLoaded(): Promise<void> {
    if (this.loaded) return;
    if (!this.loadPromise) {
      this.loadPromise = this.refresh().finally(() => {
        this.loadPromise = null;
      });
    }
    return this.loadPromise;
  }

  /** Refetch all three catalogs (for server-side creates we can't track). */
  async refresh(): Promise<void> {
    const [t, p, s] = await Promise.all([
      tagsApi.list(),
      performersApi.list(),
      studiosApi.list(),
    ]);
    this.tags = t;
    this.performers = p;
    this.studios = s;
    this.loaded = true;
  }

  async createTag(name: string): Promise<TagRow> {
    const row = await tagsApi.create(name);
    this.tags = [...this.tags.filter((t) => t.id !== row.id), row].sort(byName);
    return row;
  }

  async deleteTag(id: string): Promise<void> {
    await tagsApi.delete(id);
    this.tags = this.tags.filter((t) => t.id !== id);
  }

  async createPerformer(name: string): Promise<PerformerRow> {
    const row = await performersApi.create(name);
    this.performers = [
      ...this.performers.filter((p) => p.id !== row.id),
      row,
    ].sort(byName);
    return row;
  }

  // No deletePerformer/createStudio helpers — nothing calls them today
  // (studios/performers are created server-side via identify flows; those
  // callers use catalogs.refresh() after apply). Re-add when a UI needs them.
}

export const catalogs = new CatalogsStore();
