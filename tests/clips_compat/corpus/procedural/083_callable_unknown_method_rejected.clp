(defgeneric bad)
(defmethod bad ((?x INTEGER)) (+ 1 (missing-call ?x)))
