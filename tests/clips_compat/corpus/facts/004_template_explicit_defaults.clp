; Omitted template slots receive their declared default values.
;; Level: basic
;; Covers: facts, template-explicit-defaults
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot code (default unset)) (slot count (default 4)))
(deffacts input (item))
(defrule observe
  (item (code ?code) (count ?count))
  => (printout t ?code " " ?count crlf))
