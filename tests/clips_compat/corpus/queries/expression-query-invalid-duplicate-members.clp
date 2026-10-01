(deftemplate item (slot value))
(defrule probe => (printout t (find-all-facts ((?f item) (?f item)) TRUE) crlf))
