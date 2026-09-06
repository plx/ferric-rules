; RH-CORE-007: failed live-template redefinition preserves facts and original slot layout.
(deftemplate record (slot original (type INTEGER)))
(deffacts seed (record (original 9)))
