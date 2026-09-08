(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (marker))
(defrule probe => (do-for-fact ((?f item)) TRUE (progn$ (?f (create$ invalid-target)) (retract ?f))))
