; A negated conjunction is blocked only by a complete correlated match.
;; Level: interaction
;; Covers: patterns, ncc-join-correlation
; Protocol: load, reset, run to quiescence.
(deffacts input (owner a) (left a x) (right a y))
(defrule observe
  (owner ?owner) (not (and (left ?owner ?key) (right ?owner ?key)))
  => (printout t ?owner " clear" crlf))
