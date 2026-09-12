; Newly asserted facts propagate through successive rules in one run.
;; Level: interaction
;; Covers: agenda, forward-chain
; Protocol: load, reset, run to quiescence.
(deffacts input (phase first))
(defrule first (phase first) => (printout t "first" crlf) (assert (phase second)))
(defrule second (phase second) => (printout t "second" crlf) (assert (phase third)))
(defrule third (phase third) => (printout t "third" crlf))
