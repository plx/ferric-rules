; Breaking the only full negated-conjunction match enables its rule.
;; Level: interaction
;; Covers: patterns, ncc-retraction-unblocks
; Protocol: load, reset, run to quiescence.
(deffacts input (owner a) (left a x) (right a x))
(defrule remove-right
  (declare (salience 10)) ?f <- (right a x) => (retract ?f))
(defrule observe
  (owner ?owner) (not (and (left ?owner ?key) (right ?owner ?key)))
  => (printout t ?owner " clear" crlf))
