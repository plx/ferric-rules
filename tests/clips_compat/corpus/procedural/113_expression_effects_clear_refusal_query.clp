(deftemplate p (slot v))
(deffacts seed (p (v 1)) (p (v 2)))
(defrule run =>
  (do-for-fact ((?x p)) TRUE
    (printout t "before:" ?x:v crlf)
    (clear)
    (printout t "after:" ?x:v ":" (fact-index ?x) crlf))
  (printout t "remaining:" (length$ (find-all-facts ((?f p)) TRUE)) crlf))
