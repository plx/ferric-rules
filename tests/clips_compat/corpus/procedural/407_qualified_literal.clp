(deftemplate p (slot x))
(defrule r => (find-all-facts ((?f MAIN::p)) TRUE))
