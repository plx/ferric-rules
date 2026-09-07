; Forall succeeds when every domain element has a matching witness.
;; Level: boundary
;; Covers: patterns, forall-complete
; Protocol: load, reset, run to quiescence.
(deffacts input (item a) (item b) (done a) (done b) (done c))
(defrule observe
  (forall (item ?item) (done ?item))
  => (printout t "complete" crlf))
