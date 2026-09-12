; Forall is true when its quantified domain is empty.
;; Level: boundary
;; Covers: patterns, forall-vacuous
; Protocol: load, reset, run to quiescence.
(defrule observe
  (forall (item ?item) (done ?item))
  => (printout t "vacuous" crlf))
