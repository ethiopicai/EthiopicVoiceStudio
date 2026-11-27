import io
import unittest
from dictation_worker import AudioQueue, consume, read_messages, checked_words, MAX_QUEUED


def word(text, start=0, end=.1):
    return {'word': text, 'start': start, 'end': end}


class DictationTests(unittest.TestCase):
    def test_queue_coalesces_every_waiting_piece_without_dropping_samples(self):
        queue = AudioQueue()
        queue.push([.1] * 1600)
        queue.push([.2] * 3200)
        queue.finish()
        audio, done = queue.take()
        self.assertTrue(done)
        self.assertEqual(len(audio), 4800)
        self.assertAlmostEqual(float(audio[1599]), .1)
        self.assertAlmostEqual(float(audio[1600]), .2)

    def test_backlog_and_input_bounds_reject_instead_of_silently_dropping(self):
        queue = AudioQueue()
        for _ in range(MAX_QUEUED // 32000):
            queue.push([0.] * 32000)
        with self.assertRaisesRegex(ValueError, 'backlog_exceeded'):
            queue.push([0.])
        for invalid in [[], [float('nan')], [1.1], [True], ['0']]:
            with self.assertRaises(ValueError):
                AudioQueue().push(invalid)

    def test_explicit_finish_is_required_and_protocol_is_bounded(self):
        failures = []
        queue = AudioQueue()
        read_messages(queue, io.BytesIO(b'{"type":"audio","samples":[0.1]}\n{"type":"finish"}\n'), failures.append)
        self.assertEqual(failures, [])
        self.assertTrue(queue.finished)
        read_messages(AudioQueue(), io.BytesIO(b''), failures.append)
        self.assertEqual(failures, ['parent_closed'])
        read_messages(AudioQueue(), io.BytesIO(b'{"type":"finish","unknown":true}\n'), failures.append)
        self.assertEqual(failures[-1], 'worker_failed')

    def test_finish_commits_tail_and_events_append_final_replace_partial(self):
        queue = AudioQueue()
        queue.push([0.] * 16000)
        queue.finish()
        class Stream:
            def feed(self, audio):
                self.feed_count = len(audio)
                return {'final': [word('settled')], 'partial': [word('tentative', .2, .4)]}
            def finish(self):
                self.finished = True
                return {'final': [word('corrected-tail', .2, .4)], 'partial': []}
        stream, events = Stream(), []
        final, count = consume(stream, queue, lambda **event: events.append(event))
        self.assertTrue(stream.finished)
        self.assertEqual(count, 16000)
        self.assertEqual([w['word'] for w in final], ['settled', 'corrected-tail'])
        self.assertEqual(events[-1]['partial'], [])
        self.assertEqual(events[0]['final'], [word('settled')])
        self.assertEqual(events[-1]['final'], [word('corrected-tail', .2, .4)])

    def test_invalid_output_alignment_is_not_published(self):
        for invalid in [word('bad', -1, 0), word('bad', 0, 2), word('', 0, .1), word('bad', 0, float('nan'))]:
            with self.assertRaises(ValueError):
                checked_words([invalid], 1)

    def test_rolling_context_overlap_keeps_finals_immutable_and_saves_the_same_spans(self):
        queue = AudioQueue()
        queue.push([0.] * 16000)
        queue.finish()
        class Stream:
            def feed(self, audio):
                return {'final': [word('settled', .1, .5)],
                        'partial': [word('provisional', .45, .9)]}
            def finish(self):
                # A later context decode shifts both its spelling and start.
                return {'final': [word('context', .1, .4), word('new', .42, .9)],
                        'partial': []}
        events = []
        final, count = consume(Stream(), queue, lambda **event: events.append(event))
        self.assertEqual(count, 16000)
        self.assertEqual(events[0]['final'], [word('settled', .1, .5)])
        self.assertEqual(events[0]['partial'], [word('provisional', .5, .9)])
        self.assertEqual(events[1]['final'], [word('new', .5, .9)])
        self.assertEqual(final, [word('settled', .1, .5), word('new', .5, .9)])
        self.assertEqual(final, [w for event in events for w in event['final']])

    def test_tail_overlap_with_new_finals_is_reconciled_before_publication(self):
        queue = AudioQueue(); queue.push([0.] * 16000); queue.finish()
        class Stream:
            def feed(self, audio):
                return {'final': [word('one', 0, .6)],
                        'partial': [word('old-context', .2, .5), word('two', .55, .8)]}
            def finish(self):
                return {'final': [word('two', .58, .8)], 'partial': []}
        events = []
        final, _ = consume(Stream(), queue, lambda **event: events.append(event))
        self.assertEqual(events[0]['partial'], [word('two', .6, .8)])
        self.assertEqual(final, [word('one', 0, .6), word('two', .6, .8)])
        # Bad ordering inside a single decode remains an error.
        with self.assertRaisesRegex(ValueError, 'invalid_alignment'):
            checked_words([word('later', .5, .8), word('earlier', .1, .4)], 1)


if __name__ == '__main__':
    unittest.main()
