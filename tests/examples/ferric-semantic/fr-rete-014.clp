; FR-RETE-014: targeted undefrule removes existing activations and prevents
; future activations without damaging a live sibling that shares the same LHS.
; Internal RETE node/token reclamation and bounded churn are not observable
; through the cross-engine semantic schema and remain tracked by FR-RETE-014.
(deffacts startup
  (start)
  (subject old)
  (enabled yes))

(defrule doomed-one
  (subject ?value)
  (enabled yes)
  =>
  (printout t "doomed-one" crlf)
  (assert (result doomed-one ?value)))

(defrule doomed-two
  (subject ?value)
  (enabled yes)
  =>
  (printout t "doomed-two" crlf)
  (assert (result doomed-two ?value)))

(defrule survivor
  (subject ?value)
  (enabled yes)
  =>
  (printout t "survivor " ?value crlf)
  (assert (result survivor ?value)))

(defrule controller
  (declare (salience 100))
  ?start <- (start)
  ?subject <- (subject old)
  ?enabled <- (enabled yes)
  =>
  (undefrule doomed-one)
  (undefrule doomed-two)
  (retract ?start ?subject ?enabled)
  (assert (subject fresh))
  (assert (enabled yes))
  (assert (result controller))
  (printout t "controller" crlf))
