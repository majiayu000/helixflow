import { useEffect, useMemo, useState } from 'react';
import { fetchModelCatalog, fetchNodeCatalog, resolveImplementation } from '../../api';
import type {
  ImplementationResolution,
  ModelCatalog,
  NodeCatalog,
  WorkbenchState,
} from '../../types';

export function useCanvasCatalog() {
  const [catalog, setCatalog] = useState<NodeCatalog | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [modelCatalog, setModelCatalog] = useState<ModelCatalog | null>(null);
  const [modelCatalogError, setModelCatalogError] = useState<string | null>(null);
  const definitionByType = useMemo(
    () => new Map((catalog?.nodes ?? []).map((item) => [item.type, item] as const)),
    [catalog],
  );

  useEffect(() => {
    const controller = new AbortController();
    fetchNodeCatalog(controller.signal)
      .then((value) => {
        if (controller.signal.aborted) return;
        setCatalog(value);
        setCatalogError(null);
      })
      .catch((error) => {
        if (!controller.signal.aborted) {
          setCatalogError(error instanceof Error ? error.message : 'node catalog request failed');
        }
      });
    fetchModelCatalog(controller.signal)
      .then((value) => {
        if (controller.signal.aborted) return;
        setModelCatalog(value);
        setModelCatalogError(null);
      })
      .catch((error) => {
        if (!controller.signal.aborted) {
          setModelCatalogError(
            error instanceof Error ? error.message : 'model catalog request failed',
          );
        }
      });
    return () => controller.abort();
  }, []);

  return { catalog, catalogError, definitionByType, modelCatalog, modelCatalogError };
}

export function useImplementationResolution(
  workspaceId: string,
  providers: WorkbenchState['providers'] | undefined,
  selectedCapability: string | null,
) {
  const [resolution, setResolution] = useState<ImplementationResolution | null>(null);
  const readiness = providers?.capabilityReadiness?.find(
    (item) => item.capabilityId === selectedCapability,
  );

  useEffect(() => {
    if (!selectedCapability) {
      setResolution(null);
      return;
    }
    if (readiness && !readiness.runnable) {
      setResolution({
        status: 'unresolvable',
        code: readiness.code ?? 'PROVIDER_UNAVAILABLE',
        message: readiness.message ?? '当前 provider 不可运行该能力',
        recoverable: true,
      });
      return;
    }
    if (readiness?.mode === 'direct') {
      setResolution(null);
      return;
    }
    const controller = new AbortController();
    setResolution(null);
    resolveImplementation(
      providers?.capabilityReadiness ? workspaceId : undefined,
      selectedCapability,
      undefined,
      controller.signal,
    )
      .then((value) => {
        if (!controller.signal.aborted) setResolution(value);
      })
      .catch((error) => {
        if (controller.signal.aborted) return;
        setResolution({
          status: 'unresolvable',
          code: 'UNKNOWN',
          message: error instanceof Error ? error.message : '解析请求失败',
          recoverable: false,
        });
      });
    return () => controller.abort();
  }, [providers?.selectedProvider, readiness, selectedCapability, workspaceId]);

  return { readiness, resolution };
}
