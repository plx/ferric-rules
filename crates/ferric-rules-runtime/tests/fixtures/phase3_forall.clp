;; Phase 3 fixture: forall with multiple items
;; This fixture will be enabled when forall semantics land.
;;
;; Expected behavior:
;; - forall fires when ALL items satisfy the condition
;; - forall does NOT fire when any item fails the condition

(defrule all-checked
   (forall (item ?id) (checked ?id))
   =>
   (assert (all-complete)))

(deffacts startup
   (item 1)
   (item 2)
   (item 3)
   (checked 1)
   (checked 2)
   (checked 3))
