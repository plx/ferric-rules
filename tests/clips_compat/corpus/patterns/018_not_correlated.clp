; A negative pattern must only block the matching outer binding.
;; Level: interaction
;; Covers: patterns, not-correlated
; Protocol: load, reset, run to quiescence.
(deffacts input (item a) (item b) (blocked a))
(defrule observe
  (item ?item) (not (blocked ?item))
  => (printout t ?item crlf))
