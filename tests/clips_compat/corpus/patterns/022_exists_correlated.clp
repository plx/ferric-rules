; Exists correlates witnesses with the preceding positive binding.
;; Level: interaction
;; Covers: patterns, exists-correlated
; Protocol: load, reset, run to quiescence.
(deffacts input (owner a) (owner b) (pet a x) (pet a y))
(defrule observe
  (owner ?owner) (exists (pet ?owner ?pet))
  => (printout t ?owner crlf))
