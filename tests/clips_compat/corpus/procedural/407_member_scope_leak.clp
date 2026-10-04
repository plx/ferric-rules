(deftemplate p)
(defrule r => (find-all-facts ((?f p)) TRUE) (printout t ?f))
