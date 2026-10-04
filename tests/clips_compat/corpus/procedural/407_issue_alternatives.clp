(deftemplate p (slot x))
(deftemplate q (slot x))
(deffacts d (p (x 1)) (q (x 2)) (p (x 3)))
(defrule r => (do-for-all-facts ((?f p q)) TRUE (printout t ?f:x crlf)))
