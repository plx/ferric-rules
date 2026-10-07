(deftemplate record (slot value))
(defrule marker (declare (salience (progn (assert (record (value 7))) 2))) (never) =>)
(deftemplate captured
  (slot value (default (fact-slot-value (nth$ 1 (find-fact ((?f record)) TRUE)) value))))
(deffacts seed (captured))
(defrule report (captured (value ?value)) =>
  (printout t ?value ":" (length$ (find-all-facts ((?r record)) TRUE)) crlf))
