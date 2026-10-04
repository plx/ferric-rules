(deftemplate holder (slot ref (type FACT-ADDRESS)) (slot other (type FACT-ADDRESS)))
(defrule seed (declare (salience 10)) => (assert (holder)))
(defrule run (holder (ref ?ref) (other ?other)) =>
  (printout t ?ref ":" (eq ?ref ?other) ":" (integerp ?ref) ":" (numberp ?ref) crlf)
  (printout t (fact-index ?ref) ":" (fact-existp ?ref) ":" (fact-relation ?ref) ":" (fact-slot-names ?ref) ":" (fact-slot-value ?ref missing) crlf)
  (retract ?ref)
  (printout t "continued" crlf))
