import { useSyncExternalStore } from 'react';
import { engine, type EngineState } from './engine';

/** React subscription to the shared engine state. */
export function useEngine(): EngineState {
  return useSyncExternalStore(engine.subscribe, engine.getState, engine.getState);
}
