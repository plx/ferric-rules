(deftemplate p (slot x))
(deffacts d (p (x 1)) (p (x 2)) (want p))
(defrule r (want ?t) => (printout t (length$ (find-all-facts ((?f ?t)) TRUE)) crlf))
