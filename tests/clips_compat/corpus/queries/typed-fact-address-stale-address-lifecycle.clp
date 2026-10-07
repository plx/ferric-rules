(deftemplate item (slot v))
(defglobal ?*old* = FALSE)
(deffacts seed (item (v 1)))
(defrule replace (declare (salience 10)) ?f <- (item (v 1)) =>
  (bind ?*old* ?f)
  (retract ?f)
  (assert (item (v 2))))
(defrule inspect ?fresh <- (item (v 2)) =>
  (printout t ?*old* ":" (fact-index ?*old*) ":" (fact-existp ?*old*) crlf)
  (printout t (fact-relation ?*old*) ":" (fact-slot-names ?*old*) ":" (fact-slot-value ?*old* v) crlf)
  (printout t ?fresh ":" (fact-index ?fresh) ":" (eq ?*old* ?fresh) crlf)
  (retract ?*old*)
  (printout t "continued" crlf))
