(deftemplate p (slot x (allowed-strings "yes")))
(defrule bad => (assert (p (x "no"))))
