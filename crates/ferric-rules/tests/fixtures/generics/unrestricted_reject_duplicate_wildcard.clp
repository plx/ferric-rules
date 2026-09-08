;; Issue #323: CLIPS rejects this method parameter declaration.
(defgeneric invalid)
(defmethod invalid ($?a $?b) 1)
