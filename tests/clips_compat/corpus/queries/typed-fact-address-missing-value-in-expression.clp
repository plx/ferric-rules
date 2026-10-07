(deftemplate item (slot v))
(defrule run =>
  (printout t (eq (fact-slot-value 9 v) FALSE) crlf)
  (printout t (create$ before (fact-slot-value 9 v) after) crlf)
  (printout t "continued" crlf))
