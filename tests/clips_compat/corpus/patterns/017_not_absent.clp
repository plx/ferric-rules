; A standalone not CE succeeds when its pattern has no match.
;; Level: boundary
;; Covers: patterns, not-absent
; Protocol: load, reset, run to quiescence.
(defrule observe (not (blocked)) => (printout t "clear" crlf))
