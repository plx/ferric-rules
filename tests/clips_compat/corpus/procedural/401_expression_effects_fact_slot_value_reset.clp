(deftemplate item (slot value))
(deffunction helper () (reset) (assert (stop) (item (value 20))) value)
(defrule start (not (stop)) => (assert (item (value 99))))
(defrule r ?old <- (item (value 99))
  =>
  (printout t "fsv " (fact-slot-value ?old (helper)) " exist " (fact-existp ?old) crlf))
