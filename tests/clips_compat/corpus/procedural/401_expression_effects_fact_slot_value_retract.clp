(deftemplate item (slot value))
(deffunction helper (?f) (retract ?f) (assert (stop) (item (value 20))) value)
(defrule start (not (stop)) => (assert (item (value 99))))
(defrule r ?old <- (item (value 99))
  =>
  (printout t "fsv " (fact-slot-value ?old (helper ?old)) " exist " (fact-existp ?old) crlf))
