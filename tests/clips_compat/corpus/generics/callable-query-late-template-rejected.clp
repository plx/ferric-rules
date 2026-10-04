(defmethod rejected ((?x (any-factp ((?f later)) TRUE))) yes)
(deftemplate later (slot value))
