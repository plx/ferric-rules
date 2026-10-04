(deftemplate item (slot v))
(deftemplate holder (slot ref (type FACT-ADDRESS)))
(deffacts seed (item (v 1)))
(defrule store (declare (salience 10)) ?f <- (item) =>
  (assert (holder (ref ?f)))
  (assert (links ?f ?f))
  (retract ?f))
(defrule inspect (holder (ref ?ref)) (links ?a ?b) =>
  (printout t ?ref ":" (eq ?ref ?a) ":" (eq ?a ?b) ":" (fact-existp ?ref) crlf))
