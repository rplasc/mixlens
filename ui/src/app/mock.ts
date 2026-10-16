// Browser-only preview backend. Reads sample data from /mock/data.json (not shipped;
// generate it with the CLI). Never loaded inside the desktop app.

let data: any = null;
async function load() {
  if (data) return data;
  try {
    data = await (await fetch('mock/data.json')).json();
  } catch {
    data = { projects: [], versions: {}, reports: {}, compare: null, chat: {} };
  }
  return data;
}

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function mockInvoke(cmd: string, args: Record<string, any>): Promise<unknown> {
  const d = await load();
  switch (cmd) {
    case 'list_projects': return d.projects;
    case 'list_versions': return d.versions[args['projectId']] ?? [];
    case 'get_report': return d.reports[args['id']];
    case 'compare_versions': await delay(300); return d.compare;
    case 'section_analysis': return d.section ?? null;
    case 'codec_check': await delay(600); return d.codec ?? [];
    case 'get_settings':
      return d.settings ?? { provider: 'anthropic', anthropic_model: 'claude-sonnet-5-5', openai_model: '', ollama_model: '', ollama_url: 'http://localhost:11434', ffmpeg_path: '', main_platform: 'spotify', target_lufs: -14, effective_target_lufs: -14, max_tool_rounds: 8, has_anthropic_key: false, has_openai_key: false, ffmpeg_found: true, keyring_ok: true };
    case 'get_chat': return d.chat[args['projectId']] ?? [];
    case 'list_genre_presets': return d.presets ?? [];
    case 'update_project_context': return { ...d.projects.find((p: any) => p.id === args['id']), genre: args['genre'], key: args['key'] };
    case 'update_version_context': return null;
    case 'update_version_notes': {
      const v = Object.values(d.versions as Record<string, any[]>).flat().find((x) => x.id === args['id']);
      if (v) v.notes = args['notes'];
      return v ?? null;
    }
    case 'locate_peaks': return null;
    // The player needs decoded audio from the desktop app.
    case 'playback_audio': return null;
    case 'list_platforms': return d.platforms ?? [];
    case 'streaming_report': return d.streaming ?? null;
    case 'compare_reference': return d.reference_compare ?? null;
    case 'list_models': return ['claude-sonnet-5-5', 'claude-opus-5-5', 'claude-haiku-5-5'];
    case 'ai_chat':
      await delay(800);
      return { id: Date.now(), role: 'assistant', content: 'Preview mode: the assistant runs inside the desktop app.', created_at: '' };
    default: return null;
  }
}
