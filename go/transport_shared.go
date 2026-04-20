package haiai

import (
	"context"
	"encoding/json"
	"log"
	"sync"
)

type streamLifecycle struct {
	handleID    uint64
	events      chan AgentEvent
	done        chan struct{}
	cancel      context.CancelFunc
	closeHandle func(uint64)
	closeOnce   sync.Once
}

func openStreamLifecycle(
	ctx context.Context,
	connect func() (uint64, error),
	nextEvent func(uint64) (json.RawMessage, error),
	closeHandle func(uint64),
	logPrefix string,
) (*streamLifecycle, error) {
	handleID, err := connect()
	if err != nil {
		return nil, mapFFIErr(err)
	}

	ctx, cancel := context.WithCancel(ctx)
	lifecycle := &streamLifecycle{
		handleID:    handleID,
		events:      make(chan AgentEvent, 16),
		done:        make(chan struct{}),
		cancel:      cancel,
		closeHandle: closeHandle,
	}

	go runStreamReadLoop(ctx, lifecycle, nextEvent, logPrefix)
	return lifecycle, nil
}

func runStreamReadLoop(
	ctx context.Context,
	lifecycle *streamLifecycle,
	nextEvent func(uint64) (json.RawMessage, error),
	logPrefix string,
) {
	defer func() {
		lifecycle.closeOnce.Do(func() {
			lifecycle.closeHandle(lifecycle.handleID)
		})
		close(lifecycle.done)
		close(lifecycle.events)
	}()

	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		raw, err := nextEvent(lifecycle.handleID)
		if err != nil {
			log.Printf("[haiai] %s event error: %v", logPrefix, err)
			return
		}
		if raw == nil {
			return
		}

		var event AgentEvent
		if err := json.Unmarshal(raw, &event); err != nil {
			log.Printf("[haiai] %s unmarshal error: %v", logPrefix, err)
			continue
		}

		select {
		case lifecycle.events <- event:
		case <-ctx.Done():
			return
		default:
			log.Printf(
				"[haiai] WARNING: %s event dropped (channel full, buffer=%d): type=%s",
				logPrefix,
				cap(lifecycle.events),
				event.Type,
			)
		}
	}
}

func (lifecycle *streamLifecycle) close() {
	lifecycle.cancel()
	lifecycle.closeOnce.Do(func() {
		lifecycle.closeHandle(lifecycle.handleID)
	})
	<-lifecycle.done
}
