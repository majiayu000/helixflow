import { useEffect, useMemo, useState } from 'react';
import {
  fetchImageProcessingCapabilities,
  type ImageProcessingProfile,
} from './api-image-processing';
import type { ImageProcessingJob } from './types';

export function useImageProcessingProfile(
  workspaceId: string,
  intent: ImageProcessingJob['intent'],
) {
  const [profiles, setProfiles] = useState<ImageProcessingProfile[]>([]);
  const [profile, setProfile] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    setProfiles([]);
    setProfile('');
    void fetchImageProcessingCapabilities(workspaceId, controller.signal)
      .then((capabilities) => {
        const nextProfiles = capabilities.profiles[intent] ?? [];
        if (nextProfiles.length === 0) throw new Error(`Helixflow 没有配置 ${intent}`);
        const preferred = capabilities.defaults[intent];
        const selected = nextProfiles.some((item) => item.name === preferred)
          ? preferred as string
          : nextProfiles[0].name;
        setProfiles(nextProfiles);
        setProfile(selected);
      })
      .catch((caught) => {
        if (controller.signal.aborted) return;
        setError(caught instanceof Error ? caught.message : '读取图片模型失败');
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [intent, workspaceId]);

  const selected = useMemo(
    () => profiles.find((item) => item.name === profile) ?? null,
    [profile, profiles],
  );

  return { error, loading, profile, profiles, selected, setProfile };
}
