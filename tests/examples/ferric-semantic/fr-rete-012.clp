; FR-RETE-012 primary stage: template t is already referenced by a live rule.
(deftemplate t
  (slot x))

(defrule baseline
  =>
  (printout t "baseline" crlf)
  (assert (result baseline)))

(defrule observe-t
  (t (x ?value))
  =>
  (printout t "value=" ?value crlf)
  (assert (result ?value)))
