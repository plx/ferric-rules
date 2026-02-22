(defgeneric describe)
(defmethod describe ((?x INTEGER)) (format nil "int:%d" ?x))
(defmethod describe ((?x FLOAT)) (format nil "float:%g" ?x))
