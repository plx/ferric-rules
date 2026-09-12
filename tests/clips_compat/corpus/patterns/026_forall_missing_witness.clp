; A single domain element without a witness makes forall false.
;; Level: boundary
;; Covers: patterns, forall-missing-witness
; Protocol: load, reset, run to quiescence.
(deffacts input (item a) (item b) (done a) (ready))
(defrule incorrect
  (forall (item ?item) (done ?item)) => (printout t "incorrect" crlf))
(defrule observe (ready) => (printout t "incomplete" crlf))
