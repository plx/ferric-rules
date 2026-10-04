(deftemplate item (slot v))
(deftemplate holder (slot ref (type FACT-ADDRESS)))
(deffacts seed (item (v 1)))
(defgeneric kind)
(defmethod kind ((?x INTEGER)) integer)
(defmethod kind ((?x FACT-ADDRESS)) address)
(defrule make-holder (declare (salience 10)) => (assert (holder)))
(defrule run ?f <- (item) (holder (ref ?dummy)) =>
  (printout t (kind ?f) ":" (kind (fact-index ?f)) ":" (kind ?dummy) crlf))
